pub mod http {
    use std::{iter, str};
    use bytes::Bytes;
    use reqwest::blocking::Client;
    use crate::sha::Sha;

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[derive(Clone,Debug)]
    pub struct Ref {
        pub id: Sha,
        pub name: String
    }

    pub fn get_advertised_refs(http: &Client, url: &str, refs_only: bool) -> R<Vec<Ref>> {
        fn pkt_lines(bytes: Bytes) -> impl Iterator<Item = String> {
            let mut bytes = bytes;
            iter::from_fn(move || {
                if bytes.len() == 0 { return None; }
                let pkt_data = parse_pkt_line(&mut bytes)
                    .expect("Failed to parse pkt-line.");
                Some(pkt_data)
            })
        }

        fn parse_ref(pkt_line: String) -> R<Ref> {
            let _ref = pkt_line.split('\0').next().unwrap();
            let mut ref_parts = _ref.split(' ');
            let id = Sha::from_str(ref_parts.next().unwrap())?;
            let name = ref_parts.next().ok_or("Failed to parse ref pkt-line.")?.to_string();
            Ok(Ref { id, name })
        }

        let url = format!("{}/info/refs?service=git-upload-pack", url.trim_end_matches('/'));

        let bytes = http.get(&url).send()?.error_for_status()?.bytes()?;

        let refs = pkt_lines(bytes)
            .skip(2) // skip command & flush-pkt
            .take_while(|pkt_line| pkt_line != "")
            .map(parse_ref)
            .filter(|r| !refs_only || r.is_err() || r.as_ref().unwrap().name != "HEAD")
            .collect::<R<Vec<_>>>()?;

        Ok(refs)
    }

    pub fn clone(http: &Client, url: &str) -> R<(Ref, Bytes)> {
        let url = url.trim_end_matches('/');
        let refs = get_advertised_refs(http, url, true)?;
        let head = refs.iter()
            .find(|r| r.name == "refs/heads/master")
            .ok_or("Remote did not advertise HEAD.")?;
        let wants = refs.iter()
            .map(|Ref {id, name: _}| format!("0032want {}\n", id))
            .collect::<Vec<_>>()
            .concat();

        let mut bytes = http.post(&[url, "/git-upload-pack"].concat())
            .header("Content-Type", "application/x-git-upload-pack-request")
            .body(format!("{}00000009done\n", wants))
            .send()?
            .error_for_status()?
            .bytes()?;

        match parse_pkt_line(&mut bytes)?.as_str() {
            "NAK" => Ok((head.clone(), bytes)),
            _     => Err("Missing 'NAK'.".into())
        }
    }

    fn parse_pkt_line(bytes: &mut Bytes) -> R<String> {
        let len = bytes.split_to(4);
        let len = str::from_utf8(&len)
            .map_err(|e| format!("Failed to read pkt-len. {}", e))?;
        let len = usize::from_str_radix(len, 16)
            .map_err(|e| format!("Failed to parse pkt-len. {}", e))?;

        let data = match len {
            0 => String::new(),
            _ => str::from_utf8(&bytes.split_to(len - 4))
                    .map_err(|e| format!("Failed to parse pkt-line data. {}", e))?
                    .trim_end_matches('\n')
                    .to_string()
        };

        Ok(data)
    }
}

pub mod fmt {
    use std::{convert::{TryInto, TryFrom}, iter, path::{Path}};
    use bytes::{buf::Buf, Bytes};
    use crate::{obj::{self, ObjType}, sha::Sha, zlib};

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[derive(Debug)]
    pub enum EntryType {
        ObjCommit   = 1,
        ObjTree     = 2,
        ObjBlob     = 3,
        ObjTag      = 4,
        ObjOfsDelta = 6,
        ObjRefDelta = 7
    }

    impl TryFrom<u8> for EntryType {
        type Error = String;
        fn try_from(val: u8) -> Result<Self, Self::Error> {
            match val {
                1 => Ok(EntryType::ObjCommit),
                2 => Ok(EntryType::ObjTree),
                3 => Ok(EntryType::ObjBlob),
                4 => Ok(EntryType::ObjTag),
                6 => Ok(EntryType::ObjOfsDelta),
                7 => Ok(EntryType::ObjRefDelta),
                _ => Err(format!("Unknown entry type {}.", val).into())
            }
        }
    }

    impl TryFrom<EntryType> for ObjType {
        type Error = String;
        fn try_from(entry_type: EntryType) -> Result<Self, Self::Error> {
            match entry_type {
                EntryType::ObjCommit   => Ok(ObjType::Commit),
                EntryType::ObjTree     => Ok(ObjType::Tree),
                EntryType::ObjBlob     => Ok(ObjType::Blob),
                EntryType::ObjTag      => Ok(ObjType::Tag),
                EntryType::ObjOfsDelta |
                EntryType::ObjRefDelta => Err(format!("Entry type {:?} is not a proper object type.", entry_type).into())
            }
        }
    }

    pub struct RawObj {
        pub obj_type: ObjType,
        pub content: Vec<u8>
    }

    #[derive(Debug)]
    enum Instr {
        Copy { start: usize, end: usize },
        Insert { data: Bytes }
    }

    pub fn unpack_objects(git_dir: &Path, pack: &mut Bytes) -> R<usize> {
        fn has_no_cont_bit(byte: &u8) -> bool { (byte & 0b10000000) == 0 }
        let mut objs = std::collections::HashMap::new();

        loop {
            let first = pack.first();
            if let None = first { break; }

            let obj_type: EntryType = ((first.unwrap() & 0b01110000) >> 4).try_into()?;
            let obj_start = pack.iter().position(has_no_cont_bit).ok_or("Never ending variable sized integer.")?;
            let _obj_props = pack.split_to(obj_start + 1);

            let deflated_len = match obj_type {
                EntryType::ObjCommit |
                EntryType::ObjTree |
                EntryType::ObjBlob => {
                    let (content, deflated_len) = zlib::inflate(pack.as_ref())?;
                    let obj = RawObj { obj_type: obj_type.try_into()?, content };
                    let id = obj::write(&git_dir, obj.obj_type, &obj.content)?;
                    objs.insert(id, obj);
                    Ok(deflated_len)
                },
                EntryType::ObjRefDelta => {
                    let base_id = Sha::from_bytes(&pack.split_to(20))?;
                    let (delta, deflated_len) = zlib::inflate(pack.as_ref())?;

                    match objs.get(&base_id) {
                        Some(base) => {
                            let content = undeltify(delta, &base.content)?;
                            let obj = RawObj { obj_type: base.obj_type, content };
                            let id = obj::write(&git_dir, obj.obj_type, &obj.content)?;
                            objs.insert(id, obj);
                        },
                        None => return Err(format!("Found delta referencing unknown base object {}.", base_id).into())
                    }

                    Ok(deflated_len)
                },
                _ => Err(format!("Unsupported entry type {:?}", obj_type))
            }?;

            pack.advance(deflated_len.try_into()?);
        }

        Ok(objs.len())
    }

    pub fn undeltify(delta: Vec<u8>, base: &[u8]) -> R<Vec<u8>> {
        let mut delta = Bytes::from(delta);
        let source_len = parse_var_int(&mut delta)? as usize;
        let target_len = parse_var_int(&mut delta)? as usize;

        if source_len != base.len() {
            return Err(format!("Delta source length ({}) did not match length of base data ({}).", source_len, base.len()).into());
        }

        let mut content = Vec::new();

        loop {
            if delta.len() == 0 { break; }

            let instr = parse_instr(&mut delta)?;

            match instr {
                Instr::Copy { start, end } => content.extend_from_slice(&base[start..end]),
                Instr::Insert { data }      => content.extend_from_slice(&data)
            };
        }

        if target_len != content.len() {
            return Err(format!("Delta target length ({}) did not match length of undeltified data ({}).", source_len, content.len()).into());
        }

        Ok(content)
    }

    fn parse_instr(bytes: &mut Bytes) -> R<Instr> {
        let instr = bytes.first().unwrap();
        if *instr == 0 {
            Err("Encountered reserved delta instruction 0x0.".into())
        } else if *instr & 0b10000000 != 0 {
            Ok(parse_copy_instr(bytes))
        } else {
            Ok(parse_insert_instr(bytes))
        }
    }

    fn parse_copy_instr(bytes: &mut Bytes) -> Instr {
        let instr = bytes.split_to(1);
        let instr = instr.first().unwrap();

        let offset = (0..4)
            .map(|i| *instr & (1 << i) != 0)
            .map(|read| {
                if read {
                    let byte = bytes.split_to(1);
                    *byte.first().unwrap()
                } else {
                    0
                }
            })
            .collect::<Vec<_>>();
        let offset = u32::from_le_bytes(offset.as_slice().try_into().unwrap()) as usize;

        let len = (4..7)
            .map(|i| *instr & (1 << i) != 0)
            .map(|read| {
                if read {
                    let byte = bytes.split_to(1);
                    *byte.first().unwrap()
                } else {
                    0
                }
            })
            .chain(iter::once(0))
            .collect::<Vec<_>>();
        let len = u32::from_le_bytes(len.as_slice().try_into().unwrap()) as usize;

        Instr::Copy { start: offset, end: offset + len }
    }

    fn parse_insert_instr(bytes: &mut Bytes) -> Instr {
        let instr = bytes.split_to(1);
        let instr = instr.first().unwrap();
        let len = instr & 0b01111111;
        let data = bytes.split_to(len as usize);
        Instr::Insert { data }
    }

    fn parse_var_int(bytes: &mut Bytes) -> R<u64> {
        fn has_no_cont_bit(byte: &u8) -> bool { (byte & 0b10000000) == 0 }

        let var_int_end = bytes.iter()
            .position(has_no_cont_bit)
            .ok_or("Never ending variable sized integer.")?;

        let var_int = bytes.split_to(var_int_end + 1).iter()
            .map(|b| (b & 0b01111111) as u64)
            .enumerate()
            .map(|(i,b)| b << 7*i)
            .fold(0, |n,b| n | b);

        Ok(var_int)
    }

    pub fn parse_header(pack: &mut Bytes) -> R<u32> {
        parse_checksum(pack)?;
        parse_signature(pack)?;
        parse_version(pack)?;
        let n = parse_obj_count(pack)?;
        Ok(n)
    }

    fn parse_checksum(pack: &mut Bytes) -> R<()> {
        let checksum = Sha::from_bytes(&pack.split_off(pack.len() - 20))?;
        let packsum = Sha::generate(&pack);

        if packsum != checksum {
            Err("Pack file corrupted: checksum mismatch.".into())
        } else {
            Ok(())
        }
    }

    fn parse_signature(pack: &mut Bytes) -> R<()> {
        let sig = pack.split_to(4);
        match sig.as_ref() {
            b"PACK" => Ok(()),
            _       => Err("Missing signature 'PACK'.".into())
        }
    }

    fn parse_version(pack: &mut Bytes) -> R<()> {
        let ver = pack.split_to(4);
        let ver = u32::from_be_bytes(ver.as_ref().try_into()?);
        match ver {
            2 => Ok(()),
            _ => Err(format!("Unsupported pack version {}. Expected version 2.", ver).into())
        }
    }

    fn parse_obj_count(pack: &mut Bytes) -> R<u32> {
        let cnt = pack.split_to(4);
        let cnt = u32::from_be_bytes(cnt.as_ref().try_into()?);
        Ok(cnt)
    }
}
