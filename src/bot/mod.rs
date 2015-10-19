pub mod regexmatch;
pub mod util;

use irc::client::data::{Command, Config, Message};
use irc::client::server::{IrcServer, NetIrcServer, Server};
use irc::client::server::utils::ServerExt;
use std::io::Error;

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
                    error!("{:?}", e);
                    // Keep on trucking.
                    Ok(())
                }
            }).find(|res| res.is_err()).map_or(Ok(()), |r| r),
            Some(Err(e)) => Err(e),
            None => Ok(())
        }
    }
}
