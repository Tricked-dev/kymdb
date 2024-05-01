use std::fs;

use async_recursion::async_recursion;
use dashmap::DashSet;
use futures::{stream::FuturesUnordered, StreamExt};
use miniz_oxide::deflate::compress_to_vec;
use once_cell::sync::{Lazy, OnceCell};
use regex::Regex;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, REFERER, USER_AGENT,
};
use reqwest::Client;
use rusqlite::Connection;
use scraper::{selectable::Selectable, ElementRef, Html, Selector};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::mpsc};

static VISITED_PAGES: Lazy<DashSet<String>> = Lazy::new(DashSet::new);

type SaveFileInfo = (String, Vec<u8>);

enum PageType {
    MemeList,
    Meme,
}
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let conn = Connection::open("./memes.db")?;

    conn.execute(
        "CREATE TABLE memes (
            href TEXT,
            desc TEXT,
            title TEXT,
            img TEXT,
            title2 TEXT,
            nsfw INTEGER,
            idx INTEGER
        )",
        (), // empty list of parameters.
    )
    .ok();

    let mut headers = HeaderMap::new();

    // Add headers to make the request look like it's coming from a real browser
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
        ),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
    headers.insert(
        ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br"),
    );

    // Create a Reqwest client with the custom headers
    let client = Client::builder().default_headers(headers).build()?;

    for page in 601..=800 {
        let res = client
            .get(format!(
                "https://knowyourmeme.com/categories/meme/page/{page}?sort=views"
            ))
            .header(
                REFERER,
                format!(
                    "https://knowyourmeme.com/categories/meme/page/{}?sort=views",
                    page - 1
                ),
            )
            .send()
            .await?;
        if !res.status().is_success() {
            println!("Failed at {page}");
            println!("{res:?}");
            println!("{}", res.text().await?)
        } else {
            println!("Success at {page}");
            test_scraping(&conn, res.text().await?, page * 16)?;
        }
    }

    Ok(())
}

fn test_scraping(conn: &Connection, document: String, index: i32) -> color_eyre::Result<()> {
    let document = Html::parse_document(&document);

    let binding = Selector::parse(".infinite td").unwrap();
    let items = document.select(&binding);

    let a = Selector::parse("a").unwrap();
    let img = Selector::parse("img").unwrap();
    let h2 = Selector::parse("h2").unwrap();
    let nsfw = Selector::parse(".label-nsfw").unwrap();
    // println!("{items:?}");
    let add_checker = Selector::parse(".ad-unit-wrapper").unwrap();
    for (idx, item) in items.into_iter().enumerate() {
        if item.select(&add_checker).count() != 0 {
            continue;
        }
        let href = item
            .select(&a)
            .next()
            .unwrap()
            .value()
            .attr("href")
            .unwrap();
        let desc = item
            .select(&img)
            .next()
            .unwrap()
            .value()
            .attr("alt")
            .unwrap();
        let title = item
            .select(&img)
            .next()
            .unwrap()
            .value()
            .attr("title")
            .unwrap();
        let img = item
            .select(&img)
            .next()
            .unwrap()
            .value()
            .attr("data-src")
            .unwrap();

        let h2s = item
            .select(&h2)
            .next()
            .unwrap()
            .text()
            .collect::<Vec<_>>()
            .join(" ");
        let h2 = h2s.trim();

        let nsfw = item.select(&nsfw).count() != 0;
        // println!("{} {nsfw}: {} {} {desc}", href, img, title);

        conn.execute(
            "INSERT INTO memes VALUES (?, ?, ?, ?, ?, ?, ?)",
            (href, desc, title, img, h2, nsfw, index + idx as i32),
        )?;
    }
    Ok(())
}
