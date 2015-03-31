#![feature(io, plugin)]
#![plugin(regex_macros)]

extern crate hyper;
extern crate irc;
extern crate regex;

mod bot {
    use irc::client::data::{Command, Config, Message};
    use irc::client::server::{IrcServer, NetIrcServer, Server};
    use irc::client::server::utils::ServerExt;
    use std::io::Error;

    pub mod util {
        use hyper::client::{Client, IntoBody};
        use hyper::status::StatusCode;
        use irc::client::data::Message;
        use std::io::{Error, ErrorKind, Read};

        pub fn collect_errors(st: Result<String, String>, s: Result<String, String>) -> Result<String, String> {
            match st {
                Ok(mut st) => {
                    if let Ok(s) = s {
                        if st.len() > 0 {
                            st.push_str("; ");
                        }

                        st.push_str(&s);
                        Ok(st)
                    } else {
                        s
                    }
                },
                Err(mut st) => {
                    if let Err(e) = s {
                        st.push('\n');
                        st.push_str(&e);
                    }

                    Err(st)
                }
            }
        }
        pub fn get_reply_target(msg: &Message) -> Option<&str> {
            if is_channel(&msg.args[0][..]) {
                Some(&msg.args[0][..])
            } else {
                msg.get_source_nickname()
            }
        }
        pub fn http_get(url: &str) -> Result<String, Error> {
            let mut resp = match Client::new().get(url).send() {
                Ok(resp) => resp,
                Err(e) => return Err(Error::new(ErrorKind::Other, "Connection failed.", Some(format!("{:?}", e))))
            };

            let mut body = match resp.status {
                StatusCode::Ok => resp.into_body(),
                StatusCode::BadRequest => return Err(Error::new(ErrorKind::Other, "HTTP Bad Request", None)),
                StatusCode::NotFound => return Err(Error::new(ErrorKind::Other, "HTTP Not Found", None)),
                _ => return Err(Error::new(ErrorKind::Other, "HTTP Error", Some(format!("{:?}", resp.status))))
            };

            let mut body_str = String::new();
            try!(body.read_to_string(&mut body_str));
            Ok(body_str)
        }
        pub fn is_channel(s: &str) -> bool {
            s.chars().take(1).next().map(|c| c == '#' || c == '&').unwrap_or(false) &&
                s.chars().skip(1).filter(|c| *c == ' ' || *c == '\u{0007}' || *c == ',').next().is_none()
        }
    }
    pub mod websiteinfo {
        use irc::client::data::{Command, Message};
        use regex::Regex;
        //use std::collections::HashMap;
        use std::io::{Error, ErrorKind};
        use super::Subscriber;
        use super::util::{collect_errors, get_reply_target, http_get};

        pub struct WebsiteInfo;

        fn handle_yt<'a>(this: &mut WebsiteInfo, id: &str) -> Result<String, Error> {
            Ok("".to_string())
        }
        static URL_RES: &'static [(Regex, fn(&mut WebsiteInfo, &str) -> Result<String, Error>)] =
            &[(regex!(r"(?:(?:youtube\.com/watch\?\S*?v=)|(?:youtu\.be/))([\w-]+)"), handle_yt)];

        impl WebsiteInfo {
            pub fn new() -> WebsiteInfo {
                WebsiteInfo
            }
        }

        impl Subscriber for WebsiteInfo {
            fn on_message<'a>(&mut self, msg: &'a Message) -> Result<Option<Command>, Error> {
                if msg.command == "PRIVMSG" {
                    return if let Some(reply_target) = get_reply_target(msg) {
                        if let Some(ref suffix) = msg.suffix {
                            match URL_RES.iter().map(|&(ref re, handler)| {
                                re
                                    .captures_iter(suffix)
                                    .filter_map(|capture| capture.at(1))
                                    .map(|capture| handler(self, capture))
                                    .map(|res| match res {
                                        Ok(res) => Ok(res),
                                        Err(e) => Err(format!("{:?}", e))
                                    })
                                    .fold(Ok(String::new()), collect_errors)
                            }).fold(Ok(String::new()), collect_errors) {
                                Ok(res) => Ok(Some(Command::PRIVMSG(reply_target.to_string(), res))),
                                Err(e) => Err(Error::new(ErrorKind::Other, "Detailing a series of errors.", Some(e)))
                            }
                        } else {
                            Err(Error::new(ErrorKind::Other, "No message text.", Some(msg.into_string())))
                        }
                    } else {
                        Err(Error::new(ErrorKind::Other, "Unable to determine reply target.", Some(msg.into_string())))
                    }
                }

                Ok(None)
            }
        }
    }

    pub struct Bot<'a> {
        server: NetIrcServer,
        subscribers: Vec<&'a mut (Subscriber + 'a)>
    }
    pub trait Subscriber {
        fn on_message<'a>(&mut self, &'a Message) -> Result<Option<Command>, Error>;
    }

    impl<'a> Bot<'a> {
        pub fn new(config: Config) -> Result<Bot<'a>, Error> {
            let server = try!(IrcServer::from_config(config));
            try!(server.identify());

            Ok(Bot {
                server: server,
                subscribers: vec![]
            })
        }
        pub fn add_subscriber(&mut self, sub: &'a mut Subscriber) {
            self.subscribers.push(sub);
        }
        pub fn loop_forever(&mut self) -> Error {
            loop {
                if let Err(e) = self.loop_once() {
                    return e;
                }
            }
        }
        pub fn loop_once(&mut self) -> Result<(), Error> {
            let &mut Bot {ref mut server, ref mut subscribers} = self;

            match server.iter().next() {
                Some(Ok(msg)) => subscribers.iter_mut().map(|sub| sub.on_message(&msg)).map(|msg| match msg {
                    Ok(Some(msg)) => server.send(msg),
                    Ok(None) => Ok(()),
                    Err(e) => {
                        let targets = server.config().channels().iter().fold(String::new(), |mut st, chan| {
                            if st.len() > 0 {
                                st.push(',');
                            }
                            st.push_str(chan);
                            st
                        });

                        //TODO: Log here
                        println!("{:?} {:?}", targets, e);

                        if targets.len() > 0 {
                            server.send(Command::PRIVMSG(targets, format!("{}", e)))
                        } else {
                            Ok(())
                        }
                    }
                }).find(|res| res.is_err()).map_or(Ok(()), |r| r),
                Some(Err(e)) => Err(e),
                None => Ok(())
            }
        }
    }
}

use bot::Bot;
use bot::websiteinfo::WebsiteInfo;
use irc::client::data::Config;

fn main() {
    let mut websiteinfo = WebsiteInfo::new();

    let mut bot = Bot::new(Config::load_utf8("config.json").unwrap()).unwrap();

    bot.add_subscriber(&mut websiteinfo);
    bot.loop_forever();
}
