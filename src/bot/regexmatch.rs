use irc::client::data::{Command, Message};
use regex::Regex;
use std::io::{Error, ErrorKind};
use super::Subscriber;
use super::util::{collate_results, get_reply_target};

pub type RegexMatcher = (Regex, Box<FnMut(&str) -> Result<String, Error>>);
pub struct RegexMatch {
    matchers: Vec<RegexMatcher>,
}

impl RegexMatch {
    pub fn new() -> RegexMatch {
        RegexMatch {
            matchers: vec![],
        }
    }

    pub fn add(&mut self, matcher: RegexMatcher) {
        self.matchers.push(matcher);
    }
}

impl Subscriber for RegexMatch {
    fn on_message<'a>(&mut self, msg: &'a Message) -> Result<Option<Command>, Error> {
        if msg.command == "PRIVMSG" {
            if let Some(reply_target) = get_reply_target(msg) {
                if let Some(ref suffix) = msg.suffix {
                    // Collate the results from running the handlers on the message.
                    match self.matchers.iter_mut().map(|&mut (ref re, ref mut handler)| {
                        // Runs the regular expression on the message, and if there is a match then call the handler, collating all results.
                        re
                            .captures_iter(suffix)
                            .filter_map(|capture| capture.at(1))
                            .map(|capture| match handler(capture) {
                                Ok(res) => Ok(res),
                                Err(e) => Err(format!("{:?}", e))
                            })
                            .fold(Ok(String::new()), collate_results)
                    }).fold(Ok(String::new()), collate_results) {
                        Ok(res) => Ok(Some(Command::PRIVMSG(reply_target.to_owned(), res))),
                        Err(e) => Err(Error::new(ErrorKind::Other, e))
                    }
                } else {
                    Err(Error::new(ErrorKind::Other, "No message text."))
                }
            } else {
                Err(Error::new(ErrorKind::Other, "Unable to determine reply target."))
            }
        } else {
            Ok(None)
        }
    }
}
