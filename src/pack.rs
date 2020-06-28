pub mod http {
    use std::{iter, str};
    use bytes::Bytes;
    use reqwest::blocking::Client;

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[derive(Clone,Debug)]
    pub struct Ref {
        pub id: String,
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
            let id = ref_parts.next().unwrap().to_string();
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
    use std::{convert::TryInto, iter};
    use bytes::Bytes;
    use crate::obj::ObjType;
    use crate::sha;

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    pub struct RawObj {
        pub obj_type: ObjType,
        pub content: Vec<u8>
    }

    #[derive(Debug)]
    enum Instr {
        Copy { start: usize, end: usize },
        Insert { data: Bytes }
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
        let checksum = pack.split_off(pack.len() - 20);
        let packsum = sha::from(&pack);

        if packsum != checksum.as_ref() {
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
