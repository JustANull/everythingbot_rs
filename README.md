# everythingbot_rs

A rewrite of my Python everythingbot to be in Rust so as to take more advantage of compile time bugfixing and avoid finicky things with having to use Python in any capacity.

## Building

This README was last updated with rustc version "rustc 1.7.0-nightly (110df043b 2015-12-13)"

Run `cargo build` from the root of this project.  
Create a `config.json` file in the root of this project. For example:

    {
        "owners": ["justanull"],
        "nickname": "test",
        "username": "test",
        "realname": "test",
        "password": "",
        "server": "127.0.0.1",
        "port": 6667,
        "use_ssl": false,
        "encoding": "UTF-8",
        "channels": ["#test"],
        "options": {}
    }

Run `cargo run` from the root of the project.
