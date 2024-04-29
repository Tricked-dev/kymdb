use std::fs;

use async_recursion::async_recursion;
use dashmap::DashSet;
use futures::{stream::FuturesUnordered, StreamExt};
use miniz_oxide::deflate::compress_to_vec;
use once_cell::sync::{Lazy, OnceCell};
use regex::Regex;
use scraper::{Html, Selector};
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

    let (tx, mut rx) = mpsc::unbounded_channel::<SaveFileInfo>();
    // https://knowyourmeme.com/categories/meme/page/2?sort=views

    tokio::spawn(async move {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true) // Create the file if it doesn't exist
            .open("db.kyml")
            .await
            .expect("Failed to open file");
        while let Some(msg) = rx.recv().await {
            let mut body = vec![];
            body.extend(msg.0.as_bytes());
            body.push(b' ');
            body.extend(msg.1.as_slice());
            body.push(b'\n');
            file.write_all(&body).await.unwrap();
        }
    });
    let pages = 0..30;
    let futures = FuturesUnordered::new();

    for page in pages {
        futures.push(scrape_url(
            tx.clone(),
            format!("https://knowyourmeme.com/categories/meme/page/{page}?sort=views"),
            PageType::MemeList,
        ))
    }

    let _: Vec<_> = futures.collect().await;
    Ok(())
}

static REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?:https://knowyourmeme\.com/)?memes/[^/]+/?$"#).unwrap());

fn remove_kym<T: AsRef<str>>(text: T) -> String {
    text.as_ref().replace("https://knowyourmeme.com", "")
}

#[async_recursion(?Send)]
async fn scrape_url(
    tx: mpsc::UnboundedSender<SaveFileInfo>,
    href: String,
    page_type: PageType,
) -> color_eyre::Result<()> {
    let url = &match href {
        s if s.starts_with('/') => format!("https://knowyourmeme.com{}", s),
        _ => href.to_string(),
    };
    let path = remove_kym(url);

    let url_clone = url.clone();
    if VISITED_PAGES.contains(&path) {
        println!("dupe: {}", url_clone);
        return Ok(());
    }
    let inner = async move {
        VISITED_PAGES.insert(path.clone());

        println!("scraping: {}", url);

        let req = reqwest::get(url).await?;
        println!("status: {}", req.status());
        if !req.status().is_success() {
            println!("Error status: {}", url);
            println!("Body: ");
            println!("{}", req.text().await?);
            return Ok(());
        }
        let txt = req.text().await?;
        tx.send((path, serde_json::to_vec(&txt)?))?;
        let futures = FuturesUnordered::new();
        let document = Html::parse_document(&txt);
        match page_type {
            PageType::MemeList => {
                let binding = Selector::parse("a").unwrap();
                let links = document.select(&binding).clone();
                for link in links {
                    let href = link.value().attr("href").unwrap().to_owned();
                    if !REGEX.is_match(&href) {
                        continue;
                    }
                    futures.push(scrape_url(tx.clone(), href, PageType::Meme));
                }
            }
            PageType::Meme => {
                let binding = Selector::parse("a").unwrap();
                let links = document.select(&binding);
                for link in links {
                    let href = link.value().attr("href").unwrap().to_owned();
                    if !href.ends_with("/children") {
                        continue;
                    }

                    futures.push(scrape_url(tx.clone(), href, PageType::MemeList));
                }
            }
        };
        let _: Vec<_> = futures.collect().await;
        Ok(())
    };

    let res: color_eyre::Result<()> = inner.await;
    if res.is_ok() {
        return Ok(());
    } else {
        println!("Error: {:?}", res.err().unwrap());
        VISITED_PAGES.remove(&remove_kym(url_clone));
    }

    Ok(())
}
