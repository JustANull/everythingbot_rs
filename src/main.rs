extern crate fern;
extern crate hyper;
extern crate irc;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate regex;
extern crate rustc_serialize as serialize;

mod bot;

use bot::Bot;
use bot::regexmatch::RegexMatch;
use hyper::client::Client;
use hyper::status::StatusCode;
use irc::client::data::Config;
use regex::Regex;
use serialize::json;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

// Calls into the gfycat API to try to pull down gfycat titles, sizes, and NSFW status
fn gfycat_handler(cache: &mut HashMap<String, String>, id: &str) -> Result<String, Error> {
    Ok(match cache.entry(id.to_owned()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            let url = format!("http://gfycat.com/cajax/get/{}", id);

            let resp = &try!(http_get(&url))[..];
            let resp_json = match json::Json::from_str(resp) {
                Ok(resp_json) => resp_json,
                Err(e) => return Err(Error::new(ErrorKind::Other, e))
            };

            match resp_json.find("error") {
                Some(e) => {
                    let e = e.as_string().expect("gfycat API error - 'error' was not a string");

                    return Err(Error::new(ErrorKind::Other, e))
                },
                None => {}
            };

            // Panic here if we don't see what we expect, since the API shouldn't change underneath us
            let item = resp_json
                .find("gfyItem").expect("gfycat API error - could not find 'gfyItem'")
                .as_object().expect("gfycat API error - 'gfyItem' was not an object");
            let framerate = item
                .get("frameRate").expect("gfycat API error - could not find 'frameRate'")
                .as_f64().expect("gfycat API error - 'framerate' was not a f64");
            let frames = item
                .get("numFrames").expect("gfycat API error - could not find 'numFrames'")
                .as_u64().expect("gfycat API error - 'numFrames' was not a u64");
            let nsfw = item
                .get("nsfw").expect("gfycat API error - could not find 'nsfw'")
                .as_string().map_or("", |s| if s == "1" { "NSFW " } else { "" });
            let size = item
                .get("webmSize").expect("gfycat API error - could not find 'webmSize'")
                .as_u64().expect("gfycat API error - 'webmSize' was not a u64");
            let title = item
                .get("title").expect("gfycat API error - could not find 'title'")
                .as_string().map_or("<unknown>".to_owned(), |s| format!("\"{}\"", s));

            let res = format!("{}{} ({:.2} MB, {:.1} seconds)", nsfw, title, size as f64 / (2.0 * 1024.0 * 1024.0), frames as f64 / framerate);
            entry.insert(res).clone()
        }
    })
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
fn weather_handler(wapi_key: &str, loc: &str) -> Result<String, Error> {
    let url = format!("http://api.openweathermap.org/data/2.5/weather?q={}&APPID={}", loc, wapi_key);

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
fn yt_handler(gapi_key: &str, cache: &mut HashMap<String, String>, id: &str) -> Result<String, Error> {
    Ok(match cache.entry(id.to_owned()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            let url = format!("https://www.googleapis.com/youtube/v3/videos?id={}&key={}&part=snippet,contentDetails&fields=items(snippet/title,snippet/channelTitle,contentDetails/duration)", id, gapi_key);

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
fn xkcd_handler(cache: &mut HashMap<String, String>, id: &str) -> Result<String, Error> {
    Ok(match cache.entry(id.to_owned()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
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

            let res = format!("{} ({}-{}-{})", title, year, month, day);
            entry.insert(res).clone()
        }
    })
}

fn file_get(p: &Path) -> Result<Vec<u8>, Error> {
    Ok(try!(File::open(p)).bytes().map(|b| b.unwrap()).collect::<Vec<u8>>())
}
// Pulls down the content of a request to a URL as a string
pub fn http_get(url: &str) -> Result<String, Error> {
    let client = Client::new();
    let mut resp = match client.get(url).send() {
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

fn main() {
    let logger_config = fern::DispatchConfig {
        format: Box::new(|msg, level, _| format!("[{}] {}", level, msg)),
        output: vec![fern::OutputConfig::stdout(),
                     fern::OutputConfig::file(Path::new("log.log"))],
        level: log::LogLevelFilter::Warn,
    };
    if let Err(e) = fern::init_global_logger(logger_config, log::LogLevelFilter::Trace) {
        panic!("Failed to initialize logger: {}", e);
    }

    let mut regex_match = RegexMatch::new();

    match file_get(Path::new("gapi_key.dat")).map(String::from_utf8) {
        Ok(Ok(gapi_key)) => {
            let mut cache = HashMap::new();
            regex_match.add((Regex::new(r"(?:(?:youtube\.com/watch\?\S*?v=)|(?:youtu\.be/))([\w-]+)").unwrap(),
                             Box::new(move |res| yt_handler(&gapi_key, &mut cache, res))));
        },
        Ok(Err(utf_err)) => error!("gapi_key.dat formatting error: {}", utf_err),
        Err(io_err) => warn!("gapi_key.dat should contain a Google API key to use youtube functionality: {}", io_err),
    }
    match file_get(Path::new("wapi_key.dat")).map(String::from_utf8) {
        Ok(Ok(wapi_key)) => {
            regex_match.add((Regex::new(r"^!weather\s*(.+?)\s*$").unwrap(),
                             Box::new(move |res| weather_handler(&wapi_key, res))));
        },
        Ok(Err(utf_err)) => error!("wapi_key.dat formatting error: {}", utf_err),
        Err(io_err) => warn!("wapi_key.dat should contain an OpenWeatherMap API key to use weather functionality: {}", io_err),
    }

    let mut gfycat_cache = HashMap::new();
    regex_match.add((Regex::new(r"gfycat\.com/((?:[A-Z][a-z]+){3})").unwrap(),
                     Box::new(move |res| gfycat_handler(&mut gfycat_cache, res))));
    let mut xkcd_cache = HashMap::new();
    regex_match.add((Regex::new(r"xkcd\.com/(\d+)").unwrap(),
                     Box::new(move |res| xkcd_handler(&mut xkcd_cache, res))));

    let mut bot = Bot::new(Config::load(Path::new("config.json")).unwrap()).unwrap();

    bot.add_subscriber(&mut regex_match);
    bot.loop_forever();
}
