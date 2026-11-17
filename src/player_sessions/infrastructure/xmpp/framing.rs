use anyhow::{Result, bail};
use quick_xml::{Reader, events::Event};
use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf};

pub(super) enum Frame {
    Stream,
    Stanza(String),
    End,
}
#[derive(Default)]
pub(super) struct Framing {
    buffer: Vec<u8>,
    reads: usize,
}
impl Framing {
    pub async fn next(&mut self, read: &mut OwnedReadHalf) -> Result<Frame> {
        loop {
            if let Some((length, frame)) = split(&self.buffer)? {
                self.buffer.drain(..length);
                self.reads = 0;
                if let Some(frame) = frame {
                    return Ok(frame);
                }
                continue;
            }
            if self.buffer.len() >= 32768 || self.reads >= 64 {
                bail!("XMPP frame limit exceeded");
            }
            let mut chunk = [0u8; 4096];
            let length = read.read(&mut chunk).await?;
            if length == 0 {
                return Ok(Frame::End);
            }
            self.reads += 1;
            self.buffer.extend_from_slice(
                chunk
                    .get(..length)
                    .ok_or_else(|| anyhow::anyhow!("Invalid socket read length"))?,
            );
        }
    }
}
fn split(bytes: &[u8]) -> Result<Option<(usize, Option<Frame>)>> {
    let mut reader = Reader::from_reader(bytes);
    // The opening stream tag is consumed in a separate frame.
    reader.config_mut().allow_unmatched_ends = true;
    let mut depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if event.name().as_ref() == b"stream:stream" && depth == 0 {
                    return Ok(Some((
                        reader.buffer_position() as usize,
                        Some(Frame::Stream),
                    )));
                }
                depth += 1;
                if depth > 16 {
                    bail!("XMPP XML nesting limit exceeded");
                }
            }
            Ok(Event::Empty(_)) if depth == 0 => {
                let end = reader.buffer_position() as usize;
                return Ok(Some((
                    end,
                    Some(Frame::Stanza(
                        std::str::from_utf8(
                            bytes
                                .get(..end)
                                .ok_or_else(|| anyhow::anyhow!("Invalid XML offset"))?,
                        )?
                        .into(),
                    )),
                )));
            }
            Ok(Event::End(event)) => {
                if depth == 0 {
                    if event.name().as_ref() == b"stream:stream" {
                        return Ok(Some((reader.buffer_position() as usize, Some(Frame::End))));
                    }
                    bail!("Unexpected XMPP closing element");
                }
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = reader.buffer_position() as usize;
                    let stanza = std::str::from_utf8(
                        bytes
                            .get(..end)
                            .ok_or_else(|| anyhow::anyhow!("Invalid XML offset"))?,
                    )?
                    .to_owned();
                    return Ok(Some((end, Some(Frame::Stanza(stanza)))));
                }
            }
            Ok(Event::Decl(_) | Event::Comment(_)) if depth == 0 => {
                return Ok(Some((reader.buffer_position() as usize, None)));
            }
            Ok(Event::Text(text)) if depth == 0 => {
                anyhow::ensure!(
                    text.as_ref().iter().all(u8::is_ascii_whitespace),
                    "Text outside XMPP stanza"
                );
                return Ok(Some((reader.buffer_position() as usize, None)));
            }
            Ok(Event::DocType(_)) => bail!("XMPP DTD is prohibited"),
            Ok(Event::Eof) => return Ok(None),
            Err(quick_xml::Error::Syntax(_)) => return Ok(None),
            Err(quick_xml::Error::IllFormed(
                quick_xml::errors::IllFormedError::UnclosedReference,
            )) => return Ok(None),
            Err(error) => return Err(error.into()),
            _ => {}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_streams_partial_stanzas_and_coalesced_messages_with_bounded_depth() {
        assert!(matches!(
            split(b"<stream:stream xmlns:stream='http://etherx.jabber.org/streams'>").unwrap(),
            Some((_, Some(Frame::Stream)))
        ));
        assert!(split(b"<message><body>partial").unwrap().is_none());
        let xml = b"<message><body>a &amp; b</body></message><presence/>";
        let (end, Some(Frame::Stanza(stanza))) = split(xml).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!(stanza, "<message><body>a &amp; b</body></message>");
        assert!(matches!(
            split(&xml[end..]).unwrap(),
            Some((_, Some(Frame::Stanza(_))))
        ));
        assert!(split(format!("{}{}", "<x>".repeat(17), "</x>".repeat(17)).as_bytes()).is_err());
        assert!(split(b"<!DOCTYPE a [<!ENTITY b 'x'>]><a/>").is_err());
        assert!(split(b"<a></b>").is_err());
        assert!(split(b"</message>").is_err());
        assert!(matches!(
            split(b"</stream:stream>").unwrap(),
            Some((16, Some(Frame::End)))
        ));
        assert!(matches!(split(b" \n ").unwrap(), Some((_, None))));
    }
}
