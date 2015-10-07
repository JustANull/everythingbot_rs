extern crate hyper;
extern crate irc;
#[macro_use]
extern crate lazy_static;
extern crate regex;
extern crate rustc_serialize as serialize;

mod bot {
    pub mod util {
        use hyper::client::Client;
        use hyper::status::StatusCode;
        use irc::client::data::Message;
        use std::io::{Error, ErrorKind, Read};

        // Collates a series of successes or errors into a single string
        // Errors take priority over everything else and are *not* displayed alongside successes
        // Otherwise, all successes will be together, or all errors will be together
        // Reduces a state and a value into a new state
        pub fn collate_results(st: Result<String, String>, s: Result<String, String>) -> Result<String, String> {
            match st {
                Ok(mut st) => {
                    if let Ok(s) = s {
                        // Only add onto the list if the string is non-empty
                        // One might suggest Result<Option<String>, String>, but maybe another time
                        if !s.is_empty() {
                            if !st.is_empty() {
                                st.push_str("; ");
                            }

                            st.push_str(&s);
                        }

                        Ok(st)
                    } else {
                        // An error occurred, so we switch to that state
                        s
                    }
                },
                Err(mut st) => {
                    if let Err(e) = s {
                        // No need to check, since we only enter this state if we had an error previously
                        st.push_str("; ");
                        st.push_str(&e);
                    }

                    Err(st)
                }
            }
        }
        // Determines from a message where to reply
        // If the message was public to a channel, we tell the entire channel
        // Otherwise, the message might have been direct - reply directly
        pub fn get_reply_target(msg: &Message) -> Option<&str> {
            if is_channel(&msg.args[0][..]) {
                Some(&msg.args[0][..])
            } else {
                msg.get_source_nickname()
            }
        }
        // Pulls down the content of a request to a URL as a string
        pub fn http_get(url: &str) -> Result<String, Error> {
            let mut resp = match Client::new().get(url).send() {
                Ok(resp) => resp,
                Err(e) => return Err(Error::new(ErrorKind::Other, e))
            };

            match resp.status {
                StatusCode::Ok => {
                    let mut body = String::new();
                    try!(resp.read_to_string(&mut body));
                    Ok(body)
                },
                StatusCode::BadRequest => Err(Error::new(ErrorKind::Other, "HTTP Bad Request")),
                StatusCode::NotFound => Err(Error::new(ErrorKind::Other, "HTTP Not Found")),
                _ => Err(Error::new(ErrorKind::Other, format!("{:?}", resp.status)))
            }
        }
        // Determines whether a string represents an IRC channel
        pub fn is_channel(s: &str) -> bool {
            let mut chars = s.chars();

            // It isn't a channel if it's zero length
            if let Some(c) = chars.next() {
                // And it isn't a channel if it starts with something other than '#' or '&'
                if c != '#' && c != '&' {
                    return false;
                }
            } else {
                return false;
            }

            // It isn't a channel any of the remaining characters are ' ', a C-G, or a comma
            if let Some(_) = chars.filter(|&c| c == ' ' || c == '\u{0007}' || c == ',').next() {
                return false;
            }

            // We fulfilled all of the conditions above, so it is probably a channel name
            true
        }
    }
    pub mod regexmatch {
        use irc::client::data::{Command, Message};
        use regex::Regex;
        use serialize::json;
        use std::collections::HashMap;
        use std::collections::hash_map::Entry;
        use std::io::{Error, ErrorKind};
        use super::Subscriber;
        use super::util::{collate_results, get_reply_target, http_get};

        // TODO: This is really domain specific right now. Should this be refactored to contain the regular expressions to match over?
        pub struct RegexMatch {
            gapi_key: String,
            yt_cache: HashMap<String, String>
        }

        // Convert Kelvin to Celcius
        fn weather_k_to_c(k: f64) -> f64 {
            k - 273.15
        }
        // Convert Kelvin to Fahrenheit
        fn weather_k_to_f(k: f64) -> f64 {
            let c = weather_k_to_c(k);
            1.8 * c + 32.0
        }
        // Calls into the OpenWeatherMap API to try and get the weather at `loc`
        fn weather_handler(_: &mut RegexMatch, loc: &str) -> Result<String, Error> {
            let url = format!("http://api.openweathermap.org/data/2.5/weather?q={}", loc);

            let resp = &try!(http_get(&url))[..];
            let resp_json = match json::Json::from_str(resp) {
                Ok(resp_json) => resp_json,
                Err(e) => return Err(Error::new(ErrorKind::Other, e))
            };

            match resp_json.find("message") {
                Some(e) => {
                    let e = e.as_string().expect("OpenWeatherMap API error - 'message' was not a string");

                    return Err(Error::new(ErrorKind::Other, e))
                },
                None => {}
            };

            // Panic here if we don't see what we expect, since the API shouldn't change underneath us
            let name = resp_json
                .find("name").expect("OpenWeatherMap API error - could not find 'name'")
                .as_string().expect("OpenWeatherMap API error - 'name' was not a string");
            let description = resp_json
                .find("weather").expect("OpenWeatherMap API error - could not find 'weather'")
                .as_array().expect("OpenWeatherMap API error - 'weather' was not an array")[0]
                .find("description").expect("OpenWeatherMap API error - could not find 'description'")
                .as_string().expect("OpenWeatherMap API error - 'description' was not a string");
            let main = resp_json
                .find("main").expect("OpenWeatherMap API error - could not find 'main'");
            let temp_k = main
                .find("temp").expect("OpenWeatherMap API error - could not find 'temp'")
                .as_f64().expect("OpenWeatherMap API error - 'temp' was not a number");
            let humidity = main
                .find("humidity").expect("OpenWeatherMap API error - could not find 'humidity'")
                .as_f64().expect("OpenWeatherMap API error - 'humidity' was not a number");
            let wind_speed = resp_json
                .find("wind").expect("OpenWeatherMap API error - could not find 'wind'")
                .find("speed").expect("OpenWeatherMap API error - could not find 'speed'")
                .as_f64().expect("OpenWeatherMap API error - 'speed' was not a number");

            Ok(format!("{} weather: {:.2} \u{00B0}F ({:.2} \u{00B0}C), {}% humidity, {} and wind {:.2} m/s",
                       name, weather_k_to_f(temp_k), weather_k_to_c(temp_k), humidity, description, wind_speed))
        }
        // Parses an ISO 8601 duration string into a human-readable duration
        // e.g. "3M20S" -> 3 minutes 20 seconds
        // TODO: Is there a package since I made this that handles ISO time well?
        fn yt_parse_time(s: &str) -> String {
            let mut in_time = false;
            let mut start_idx: Option<usize> = None;
            let mut prev_char: char = '\u{0000}';
            let mut length = String::new();

            for (idx, c) in s.chars().enumerate() {
                match c {
                    '0' ... '9' => {prev_char = c; continue}
                    'P' => start_idx = Some(idx + 1),
                    'T' => {
                        start_idx = Some(idx + 1);
                        in_time = true
                    },
                    _ => {
                        if length.len() > 0 {
                            length.push_str(", ");
                        }

                        length.push_str(&s[start_idx.unwrap() .. idx]);
                        length.push(' ');
                        length.push_str(match c {
                            'Y' => "year",
                            'M' => if in_time {"minute"} else {"month"},
                            'D' => "day",
                            'H' => "hour",
                            'S' => "second",
                            _ => panic!("Unexpected character in ISO 8601 duration string")
                        });

                        if idx - start_idx.unwrap() > 1 || prev_char != '1' {
                            length.push('s');
                        }

                        start_idx = Some(idx + 1);
                    }
                }
            }

            length
        }
        // Calls into Google's Youtube API to determine video duration, title, and channel title, or accesses that data from a cache
        fn yt_handler(this: &mut RegexMatch, id: &str) -> Result<String, Error> {
            Ok(match this.yt_cache.entry(id.to_owned()) {
                Entry::Occupied(entry) => entry.get().clone(),
                Entry::Vacant(entry) => {
                    let url = format!("https://www.googleapis.com/youtube/v3/videos?id={}&key={}&part=snippet,contentDetails&fields=items(snippet/title,snippet/channelTitle,contentDetails/duration)", id, this.gapi_key);

                    let resp = &try!(http_get(&url))[..];
                    let resp_json = match json::Json::from_str(resp) {
                        Ok(resp_json) => resp_json,
                        Err(e) => return Err(Error::new(ErrorKind::Other, e))
                    };

                    // I chose to panic here on any errors, since if we get a response from Youtube
                    // it should be of the correct form - their API shouldn't change underneath us
                    let ref items = resp_json
                        .find("items").expect("Youtube API error - could not find 'items'")
                        .as_array().expect("Youtube API error - 'items' was not an array")[0];
                    let snippet = items
                        .find("snippet").expect("Youtube API error - could not find 'snippet'");
                    let title = snippet
                        .find("title").expect("Youtube API error - could not find 'title'")
                        .as_string().expect("Youtube API error - 'title' was not a string");
                    let channel_title = snippet
                        .find("channelTitle").expect("Youtube API error - could not find 'channelTitle'")
                        .as_string().expect("Youtube API error - 'channelTitle' was not a string");
                    let duration = items
                        .find("contentDetails").expect("Youtube API error - could not find 'contentDetails'")
                        .find("duration").expect("Youtube API error - could not find 'duration'")
                        .as_string().expect("Youtube API error - 'duration' was not a string");

                    let res = format!("\"{}\" by {} ({})", title, channel_title, yt_parse_time(duration));
                    entry.insert(res).clone()
                }
            })
        }
        // Calls into XKCD's API to determine comic name and date from a comic ID
        fn xkcd_handler(_: &mut RegexMatch, id: &str) -> Result<String, Error> {
            let url = format!("https://xkcd.com/{}/info.0.json", id);

            let resp = &try!(http_get(&url))[..];
            let resp_json = match json::Json::from_str(resp) {
                Ok(resp_json) => resp_json,
                Err(e) => return Err(Error::new(ErrorKind::Other, e))
            };

            let title = resp_json
                .find("title").expect("XKCD API error - could not find 'title'")
                .as_string().expect("XKCD API error - 'title' was not a string");
            let year = resp_json
                .find("year").expect("XKCD API error - could not find 'year'")
                .as_string().expect("XKCD API error - 'year' was not a string");
            let month = resp_json
                .find("month").expect("XKCD API error - could not find 'month'")
                .as_string().expect("XKCD API error - 'month' was not a string");
            let day = resp_json
                .find("day").expect("XKCD API error - could not find 'day'")
                .as_string().expect("XKCD API error - 'day' was not a string");

            Ok(format!("{} ({}-{}-{})", title, year, month, day))
        }

        lazy_static! {
            // The regular expressions and their handlers to match with
            static ref URL_RES: Vec<(Regex, fn(&mut RegexMatch, &str) -> Result<String, Error>)> =
                vec![(Regex::new(r"^!weather\s*(.*)\s*$").unwrap(), weather_handler),
                     (Regex::new(r"(?:(?:youtube\.com/watch\?\S*?v=)|(?:youtu\.be/))([\w-]+)").unwrap(), yt_handler),
                     (Regex::new(r"xkcd\.com/(\d+)").unwrap(), xkcd_handler)];
        }

        impl RegexMatch {
            pub fn new(gapi_key: &str) -> RegexMatch {
                RegexMatch {
                    gapi_key: gapi_key.to_owned(),
                    yt_cache: HashMap::new()
                }
            }
        }

        impl Subscriber for RegexMatch {
            fn on_message<'a>(&mut self, msg: &'a Message) -> Result<Option<Command>, Error> {
                if msg.command == "PRIVMSG" {
                    if let Some(reply_target) = get_reply_target(msg) {
                        if let Some(ref suffix) = msg.suffix {
                            // Collate the results from running the handlers on the message.
                            match URL_RES.iter().map(|&(ref re, handler)| {
                                // Runs the regular expression on the message, and if there is a match then call the handler, collating all results.
                                re
                                    .captures_iter(suffix)
                                    .filter_map(|capture| capture.at(1))
                                    .map(|capture| match handler(self, capture) {
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
    }

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
                        // In the case of an error, notify every channel we're connected to about it
                        // Errors shouldn't happen, because they indicate a failure in our code to properly
                        // handle APIs. Errors which result from program logic being incorrect (or APIs changing
                        // underneath us, which is impossible to handle anyway) are panics rather than errors
                        let targets = server.config().channels().iter().fold(String::new(), |mut st, chan| {
                            if st.len() > 0 {
                                st.push(',');
                            }
                            st.push_str(chan);
                            st
                        });

                        //TODO: Log here somewhere that isn't the console
                        println!("{:?} {:?}", targets, e);

                        if targets.len() > 0 {
                            server.send(Command::PRIVMSG(targets, format!("{}", e)))
                        } else {
                            // We aren't connected to any channels, but there was an error
                            // Such a thing might happen if it happened during a private message while connected to no channels
                            // For now, ignore that case and do nothing
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
use bot::regexmatch::RegexMatch;
use irc::client::data::Config;
use std::fs::File;
use std::io::{Error, Read};
use std::path::Path;

fn file_get(p: &Path) -> Result<Vec<u8>, Error> {
    Ok(try!(File::open(p)).bytes().map(|b| b.unwrap()).collect::<Vec<u8>>())
}

fn main() {
    let mut regex_match = RegexMatch::new(&String::from_utf8(file_get(Path::new("gapi_key.dat"))
                                                             .ok().expect("gapi_key.dat should be on the path"))
                                          .ok().expect("gapi_key.dat should contain only the Google API key (no trailing newlines)"));

    let mut bot = Bot::new(Config::load(Path::new("config.json")).unwrap()).unwrap();

    bot.add_subscriber(&mut regex_match);
    bot.loop_forever();
}
