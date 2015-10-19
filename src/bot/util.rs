use irc::client::data::Message;
use std::io::Read;

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
