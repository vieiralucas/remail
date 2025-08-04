use email_address::EmailAddress;
use std::io::{BufRead, BufReader, Lines};
use std::ops::{Index, IndexMut};
use std::str::FromStr;

#[derive(Debug, PartialEq, Clone)]
pub struct NonEmptyVec<T> {
    pub head: T,
    tail: Vec<T>,
}

impl<T> NonEmptyVec<T> {
    pub fn new(head: T) -> Self {
        Self {
            head,
            tail: Vec::new(),
        }
    }

    pub fn with_tail(head: T, tail: Vec<T>) -> Self {
        Self { head, tail }
    }

    pub fn len(&self) -> usize {
        1 + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn push(&mut self, value: T) {
        self.tail.push(value);
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.head).chain(self.tail.iter())
    }

    pub fn into_vec(self) -> Vec<T> {
        let mut v = Vec::with_capacity(1 + self.tail.len());
        v.push(self.head);
        v.extend(self.tail);
        v
    }
}

impl<T> Index<usize> for NonEmptyVec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index == 0 {
            &self.head
        } else {
            &self.tail[index - 1]
        }
    }
}

impl<T> IndexMut<usize> for NonEmptyVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index == 0 {
            &mut self.head
        } else {
            &mut self.tail[index - 1]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageParserEvent {
    From(Option<EmailAddress>),
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
    Headers,
    Body,
    End,
    Done,
}

pub struct MessageParser<R: std::io::Read> {
    lines: Lines<BufReader<R>>,
    state: MessageParserState,

    from: Option<EmailAddress>,
    to: NonEmptyVec<EmailAddress>,
    // TODO: refactor a headers parse out of here
    current_header: Option<(String, NonEmptyVec<String>)>,
    headers: Vec<(String, String)>,
    body: Vec<String>,
}

impl<R: std::io::Read> MessageParser<R> {
    pub fn new(reader: R) -> Self {
        let lines = BufReader::new(reader).lines();

        Self {
            lines,
            state: MessageParserState::Start,
            from: None,
            to: NonEmptyVec::new(EmailAddress::new_unchecked("")),
            current_header: None,
            headers: Vec::new(),
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
    UnexpectedDataAfterEnd(String),
    InvalidHeader(String),
}

fn parse_rcpt_to(line: &str) -> Result<EmailAddress, email_address::Error> {
    let to = line[8..]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or("")
        .to_string();
    EmailAddress::from_str(&to)
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

                            if from.is_empty() {
                                self.from = None;
                                self.state = MessageParserState::MailFrom;
                                return Some(Ok(MessageParserEvent::From(None)));
                            }

                            match EmailAddress::from_str(&from) {
                                Ok(email) => {
                                    self.from = Some(email.clone());
                                    self.state = MessageParserState::MailFrom;
                                    Some(Ok(MessageParserEvent::From(Some(email))))
                                }
                                Err(err) => {
                                    Some(Err(MessageParserError::InvalidFromEmailAddress(err)))
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
                            match parse_rcpt_to(&line) {
                                Ok(email) => {
                                    self.to[0] = email.clone();
                                    self.state = MessageParserState::RcptTo;
                                    Some(Ok(MessageParserEvent::To(email)))
                                }
                                Err(err) => {
                                    Some(Err(MessageParserError::InvalidToEmailAddress(err)))
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
                            self.state = MessageParserState::Headers;
                            self.next()
                        } else if line.starts_with("RCPT TO:") {
                            match parse_rcpt_to(&line) {
                                Ok(email) => {
                                    self.to.push(email.clone());
                                    self.state = MessageParserState::RcptTo;
                                    Some(Ok(MessageParserEvent::To(email)))
                                }
                                Err(err) => {
                                    Some(Err(MessageParserError::InvalidToEmailAddress(err)))
                                }
                            }
                        } else {
                            // TODO: we should actually check if this is a command that exists
                            // to return a BadSequenceOfCommands Error instead of always returning
                            // a UnrecognizedCommand Error
                            Some(Err(MessageParserError::UnrecognizedCommand(line)))
                        }
                    }
                    // TODO: we should have a headers parse
                    MessageParserState::Headers => {
                        if line.is_empty() {
                            if let Some((name, value)) = &self.current_header {
                                self.headers
                                    .push((name.clone(), value.clone().into_vec().join(" ")));
                                self.current_header = None;
                            }
                            self.state = MessageParserState::Body;
                            self.next()
                        } else if line.starts_with(" ") || line.starts_with("\t") {
                            if let Some((_, value)) = &mut self.current_header {
                                let part = line
                                    .strip_prefix(" ")
                                    .unwrap_or_default()
                                    .strip_prefix("\t")
                                    .unwrap_or_default()
                                    .to_string();
                                value.push(part);
                                self.next()
                            } else {
                                Some(Err(MessageParserError::InvalidHeader(line)))
                            }
                        } else {
                            let parsed_header = self
                                .current_header
                                .as_ref()
                                .map(|(name, value)| (name, value.clone().into_vec().join(" ")));
                            if let Some((name, value)) = parsed_header {
                                self.headers.push((name.clone(), value.clone()));
                                self.current_header = None;
                            }

                            match line.split_once(":") {
                                Some((name, rest)) => {
                                    self.current_header = Some((
                                        name.to_string(),
                                        NonEmptyVec::new(rest.to_string()),
                                    ));
                                    self.next()
                                }
                                None => {
                                    let mut raw_header = String::new();
                                    if let Some((name, value)) = &self.current_header {
                                        raw_header.push_str(name);
                                        for part in value.iter() {
                                            raw_header.push_str(&format!(" {part}"));
                                        }
                                    }
                                    raw_header.push_str(&line);
                                    Some(Err(MessageParserError::InvalidHeader(raw_header.clone())))
                                }
                            }
                        }
                    }
                    MessageParserState::Body => {
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
                        Some(Err(MessageParserError::UnexpectedDataAfterEnd(line)))
                    }
                    MessageParserState::Done => {
                        Some(Err(MessageParserError::UnexpectedDataAfterEnd(line)))
                    }
                }
            }
            Some(Err(err)) => Some(Err(MessageParserError::IO(err))),
            None => match self.state {
                MessageParserState::Start => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::Helo => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::MailFrom => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::RcptTo => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::Headers => Some(Err(MessageParserError::UnexpectedEnd)),
                MessageParserState::Body => Some(Err(MessageParserError::UnexpectedEnd)),
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
            Some(Err(err)) => assert!(false, "Expected {:?} but got error: {:?}", expected, err),
            None => assert_eq!(Some(expected), None),
        }
    }

    #[test]
    fn test_message_parser() {
        let input = "HELO example.com\r\nMAIL FROM: <test@example.com>\r\nRCPT TO: <test@example.com>\r\nDATA\r\nHello, world!\r\n.\r\n";
        let mut parser = MessageParser::new(input.as_bytes());

        assert_event(
            MessageParserEvent::From(Some(EmailAddress::new_unchecked("test@example.com"))),
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

    #[test]
    fn test_mail_from() {
        let table = vec![
            (
                "MAIL FROM: <test@example.com>",
                Some(EmailAddress::new_unchecked("test@example.com")),
            ),
            (
                "MAIL FROM:<test@example.com>",
                Some(EmailAddress::new_unchecked("test@example.com")),
            ),
            ("MAIL FROM: <>", None),
            ("MAIL FROM:<>", None),
            (
                "MAIL FROM: <test+tag@example.com>",
                Some(EmailAddress::new_unchecked("test+tag@example.com")),
            ),
            (
                "MAIL FROM: <test@example.com> param1=ignored",
                Some(EmailAddress::new_unchecked("test@example.com")),
            ),
        ];

        for (input, expected) in table {
            let input = vec!["HELO example.com", input].join("\r\n");
            let mut parser = MessageParser::new(input.as_bytes());
            let actual = parser.next();
            assert_event(MessageParserEvent::From(expected.clone()), actual);
            assert_eq!(expected, parser.from);
        }
    }

    #[test]
    fn test_rcpt_to() {
        let table = vec![
            (
                "RCPT TO: <test@example.com>",
                NonEmptyVec::new(EmailAddress::new_unchecked("test@example.com")),
            ),
            (
                "RCPT TO:<test@example.com>",
                NonEmptyVec::new(EmailAddress::new_unchecked("test@example.com")),
            ),
            (
                "RCPT TO: <test+tag@example.com>",
                NonEmptyVec::new(EmailAddress::new_unchecked("test+tag@example.com")),
            ),
            (
                "RCPT TO: <test@example.com> param1=ignored",
                NonEmptyVec::new(EmailAddress::new_unchecked("test@example.com")),
            ),
            (
                "RCPT TO: <test@example.com>\r\nRCPT TO: <test2@example.com>",
                NonEmptyVec::with_tail(
                    EmailAddress::new_unchecked("test@example.com"),
                    vec![EmailAddress::new_unchecked("test2@example.com")],
                ),
            ),
        ];

        for (input, expected) in table {
            let input = vec!["HELO example.com", "MAIL FROM:<>", input].join("\r\n");

            let mut parser = MessageParser::new(input.as_bytes());
            // skip mail from
            parser.next();

            for i in 0..(expected.len()) {
                let actual = parser.next();
                assert_event(MessageParserEvent::To(expected[i].clone()), actual);
            }

            assert_eq!(expected, parser.to);
        }
    }

    #[test]
    fn test_headers() {
        let table = vec![
            (
                "Subject: Test\r\n",
                vec![("Subject".to_string(), "Test".to_string())],
            ),
            (
                "Subject:Test\r\n",
                vec![("Subject".to_string(), "Test".to_string())],
            ),
            (
                "Subject:Test \r\n",
                vec![("Subject".to_string(), "Test ".to_string())],
            ),
            (
                "Subject: Test\r\nCc: john@example.com\r\n",
                vec![
                    ("Subject".to_string(), "Test".to_string()),
                    ("Cc".to_string(), "john@example.com".to_string()),
                ],
            ),
            (
                // leading space
                "Subject:  Test\r\n",
                vec![("Subject".to_string(), " Test".to_string())],
            ),
            (
                // folded multi line headers with empty first line
                "Subject:\r\n Test\r\n",
                vec![("Subject".to_string(), " Test".to_string())],
            ),
            (
                // folded multi line headers with leading space first line
                "Subject: \r\n Test\r\n",
                vec![("Subject".to_string(), " Test".to_string())],
            ),
            (
                // folded multi line headers with non empty first line
                "Subject: First\r\n Second\r\n",
                vec![("Subject".to_string(), "First Second".to_string())],
            ),
            (
                // folded multi line headers with empty lines in between line
                "Subject: First\r\n \r\n Second\r\n",
                vec![("Subject".to_string(), "First  Second".to_string())],
            ),
        ];

        for (input, expected) in table {
            let input = vec![
                "HELO example.com",
                "MAIL FROM: <>",
                "RCPT TO: <jane@example.com>",
                "DATA",
                input,
            ]
            .join("\r\n");

            dbg!(input.clone());

            let mut parser = MessageParser::new(input.as_bytes());

            // skip mail from
            parser.next();
            // skip rcpt to
            parser.next();

            for i in 0..(expected.len()) {
                let actual = parser.next();
                assert_event(
                    MessageParserEvent::Header(expected[i].0.clone(), expected[i].1.clone()),
                    actual,
                );
            }

            assert_eq!(expected, parser.headers);
        }
    }
}
