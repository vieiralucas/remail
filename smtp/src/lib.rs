use email_address::EmailAddress;
use std::io::{BufRead, BufReader, Lines};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageParserEvent {
    From(EmailAddress),
    To(EmailAddress),
    Header(String, String),
    Body(Vec<String>),
    Done(Message),
}

pub enum MessageParserState {
    Start,
    Helo,
    MailFrom,
    RcptTo,
    Data,
    End,
    Done,
}

pub struct MessageParser<R: std::io::Read> {
    lines: Lines<BufReader<R>>,
    state: MessageParserState,

    from: EmailAddress,
    to: EmailAddress,
    body: Vec<String>,
}

impl<R: std::io::Read> MessageParser<R> {
    pub fn new(reader: R) -> Self {
        let lines = BufReader::new(reader).lines();

        Self {
            lines,
            state: MessageParserState::Start,
            from: EmailAddress::new_unchecked(""),
            to: EmailAddress::new_unchecked(""),
            body: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub enum MessageParserError {
    IO(std::io::Error),
    UnrecognizedCommand(String),
    InvalidFromEmailAddress(email_address::Error),
    InvalidToEmailAddress(email_address::Error),
    UnexpectedEnd,
    UnexpectedDataAfterEnd,
}

impl<R: std::io::Read> Iterator for MessageParser<R> {
    type Item = Result<MessageParserEvent, MessageParserError>;

    fn next(&mut self) -> Option<Self::Item> {
        let line = self.lines.next();
        match line {
            Some(Ok(line)) => {
                match self.state {
                    MessageParserState::Start => {
                        if line.len() < 4 {
                            return Some(Err(MessageParserError::UnrecognizedCommand(line)));
                        }
                        let line = line[..4].to_uppercase();
                        if line == "HELO" || line == "EHLO" {
                            self.state = MessageParserState::Helo;
                            self.next()
                        } else {
                            Some(Err(MessageParserError::UnrecognizedCommand(line)))
                        }
                    }
                    MessageParserState::Helo => {
                        if line.len() < 10 {
                            return Some(Err(MessageParserError::UnrecognizedCommand(line)));
                        }
                        if line[..10].to_uppercase() == "MAIL FROM:" {
                            let from = line[10..]
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .strip_prefix('<')
                                .and_then(|s| s.strip_suffix('>'))
                                .unwrap_or("")
                                .to_string();

                            match EmailAddress::from_str(&from) {
                                Ok(email) => {
                                    self.from = email.clone();
                                    self.state = MessageParserState::MailFrom;
                                    Some(Ok(MessageParserEvent::From(email)))
                                }
                                Err(err) => {
                                    Some(Err(MessageParserError::InvalidFromEmailAddress(
                                        err,
                                    )))
                                }
                            }
                        } else {
                            // TODO: we should actually check if this is a command that exists
                            // to return a BadSequenceOfCommands Error instead of always returning
                            // a UnrecognizedCommand Error
                            Some(Err(MessageParserError::UnrecognizedCommand(line)))
                        }
                    }
                    MessageParserState::MailFrom => {
                        if line.len() < 8 {
                            // TODO: we should actually check if this is a command that exists
                            // to return a BadSequenceOfCommands Error instead of always returning
                            // a UnrecognizedCommand Error
                            return Some(Err(MessageParserError::UnrecognizedCommand(line)));
                        }
                        if line[..8].to_uppercase() == "RCPT TO:" {
                            let to = line[8..]
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .strip_prefix('<')
                                .and_then(|s| s.strip_suffix('>'))
                                .unwrap_or("")
                                .to_string();
                            match EmailAddress::from_str(&to) {
                                Ok(email) => {
                                    self.to = email.clone();
                                    self.state = MessageParserState::RcptTo;
                                    Some(Ok(MessageParserEvent::To(email)))
                                }
                                Err(err) => {
                                    Some(Err(MessageParserError::InvalidToEmailAddress(
                                        err,
                                    )))
                                }
                            }
                        } else {
                            // TODO: we should actually check if this is a command that exists
                            // to return a BadSequenceOfCommands Error instead of always returning
                            // a UnrecognizedCommand Error
                            Some(Err(MessageParserError::UnrecognizedCommand(line)))
                        }
                    }
                    MessageParserState::RcptTo => {
                        if line.to_uppercase() == "DATA" {
                            self.state = MessageParserState::Data;
                            self.next()
                        } else {
                            // TODO: we should actually check if this is a command that exists
                            // to return a BadSequenceOfCommands Error instead of always returning
                            // a UnrecognizedCommand Error
                            Some(Err(MessageParserError::UnrecognizedCommand(line)))
                        }
                    }
                    MessageParserState::Data => {
                        if line == "." {
                            self.state = MessageParserState::End;
                            return Some(Ok(MessageParserEvent::Body(self.body.clone())));
                        }

                        let line_to_push = if let Some(line) = line.strip_prefix(".") {
                            // Section 4.5.2 of RFC 5321 states that lines starting with a dot
                            // should have the dot removed when they are part of the message body.
                            // This is to avoid confusion with the end of data marker.
                            // So we push the line without the leading dot.
                            line.to_string()
                        } else {
                            line.to_string()
                        };

                        self.body.push(line_to_push);
                        self.next()
                    }
                    MessageParserState::End => {
                        Some(Err(MessageParserError::UnexpectedDataAfterEnd))
                    }
                    MessageParserState::Done => {
                        Some(Err(MessageParserError::UnexpectedDataAfterEnd))
                    }
                }
            }
            Some(Err(err)) => Some(Err(MessageParserError::IO(err))),
            None => match self.state {
                MessageParserState::Start => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::Helo => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::MailFrom => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::RcptTo => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::Data => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::End => Some(Ok(MessageParserEvent::Done(Message {}))),
                MessageParserState::Done => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_event(
        expected: MessageParserEvent,
        actual: Option<Result<MessageParserEvent, MessageParserError>>,
    ) {
        match actual {
            Some(Ok(event)) => assert_eq!(expected, event),
            Some(Err(err)) => assert!(false, "Unexpected error: {:?}", err),
            None => assert!(false, "Unexpected end of input"),
        }
    }

    #[test]
    fn test_message_parser() {
        let input = "HELO example.com\r\nMAIL FROM: <test@example.com>\r\nRCPT TO: <test@example.com>\r\nDATA\r\nHello, world!\r\n.\r\n";
        let mut parser = MessageParser::new(input.as_bytes());

        assert_event(
            MessageParserEvent::From(EmailAddress::new_unchecked("test@example.com")),
            parser.next(),
        );
        assert_event(
            MessageParserEvent::To(EmailAddress::new_unchecked("test@example.com")),
            parser.next(),
        );
        assert_event(
            MessageParserEvent::Body(vec!["Hello, world!".to_string()]),
            parser.next(),
        );
        assert_event(MessageParserEvent::Done(Message {}), parser.next());
    }
}
