//! Separately distributed native worker. One request on stdin, one response on stdout.
use panzo_plugin_api::{Image, MAX_MESSAGE, PROTOCOL, Request, Response};
use std::io::{Read, Write};
fn transform(request: &Request) -> Result<Image, String> {
    if request.protocol != PROTOCOL {
        return Err("unsupported protocol".into());
    }
    let mut pixels = request.image.decode()?;
    match request.command.as_str() {
        "grayscale" => {
            for p in pixels.chunks_exact_mut(4) {
                let y = ((u32::from(p[2]) * 77 + u32::from(p[1]) * 150 + u32::from(p[0]) * 29) >> 8)
                    as u8;
                p[..3].fill(y);
            }
        }
        "invert" => {
            for p in pixels.chunks_exact_mut(4) {
                for channel in &mut p[..3] {
                    *channel = 255 - *channel;
                }
            }
        }
        _ => return Err("unknown command".into()),
    }
    Image::new(request.image.width, request.image.height, &pixels)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_MESSAGE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MESSAGE {
        return Err("input too large".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let response = match transform(&request) {
        Ok(image) => Response {
            protocol: PROTOCOL,
            request_id: request.request_id,
            image: Some(image),
            error: None,
        },
        Err(error) => Response {
            protocol: PROTOCOL,
            request_id: request.request_id,
            image: None,
            error: Some(error),
        },
    };
    std::io::stdout().write_all(&serde_json::to_vec(&response)?)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grayscale_preserves_alpha_and_invert_channels() {
        let mut r = Request {
            protocol: PROTOCOL,
            request_id: "test".into(),
            document_id: 1,
            revision: 2,
            command: "grayscale".into(),
            image: Image::new(1, 1, &[0, 0, 255, 111]).unwrap(),
        };
        assert_eq!(transform(&r).unwrap().decode().unwrap(), [76, 76, 76, 111]);
        r.command = "invert".into();
        assert_eq!(transform(&r).unwrap().decode().unwrap(), [255, 255, 0, 111]);
    }
}
