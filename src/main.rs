use std::{
    cmp::min,
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use scraper::{Html, Selector};
use tokio::{fs::File, io::AsyncWriteExt, sync::Semaphore, task::JoinSet};

use futures_util::StreamExt;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    output_dir: String,

    /// Number of parallel downloads
    #[arg(short, default_value_t = 5)]
    num_of_lanes: u8,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Link {
    url: String,
    name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _args = Args::parse();

    let client = reqwest::Client::new();

    let site_html = client
        .get("https://web.sas.upenn.edu/upennidb/albums/")
        .headers(get_headers())
        .send()
        .await?
        .text()
        .await?;

    let html = Html::parse_document(&site_html);
    let mut _links = collect_links(&html)?;

    let dir = PathBuf::from(_args.output_dir);
    if !dir.exists() {
        tokio::fs::DirBuilder::new()
            .recursive(true)
            .create(&dir)
            .await?;
    }

    let mb = MultiProgress::new();
    let p_sty = ProgressStyle::default_bar()
        .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?
        .progress_chars("#>-");

    let mut join_set: JoinSet<anyhow::Result<()>> = JoinSet::new();

    let semaphore = Arc::new(Semaphore::new(_args.num_of_lanes.into()));

    for link in _links {
        let dir = dir.clone();
        let mb = mb.clone();
        let p_sty = p_sty.clone();

        let permit = Arc::clone(&semaphore).acquire_owned().await;

        join_set.spawn(async move {
            let _permit = permit;
            download_file_to(&link, dir, mb, p_sty).await
        });
    }

    while let Some(r) = join_set.join_next().await {
        println!("Result of download: {r:?}");
    }

    Ok(())
}

async fn download_file_to(
    link: &Link,
    dir: impl AsRef<Path>,
    mb: MultiProgress,
    p_sty: ProgressStyle,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    let resp = client.get(&link.url).headers(get_headers()).send().await?;

    let total_size = resp.content_length().unwrap_or(0);
    let msg: &'static str = Box::leak::<'static>(Box::new(format!("Downloading {}", link.name)));
    let pb = mb.add(ProgressBar::new(total_size));
    pb.set_style(p_sty);
    pb.set_message(msg);

    let file_path = PathBuf::from(dir.as_ref()).join(Path::new(&link.name));

    let mut file = File::create(file_path).await?;
    let mut downloaded = 0u64;
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;

        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(new);
    }

    pb.finish_with_message("done");

    Ok(())
}

fn collect_links(doc: &Html) -> anyhow::Result<BTreeSet<Link>> {
    let selector =
        Selector::parse(".nidb-album a").expect("Could not create '.nidb-album a' selector");
    let urls = doc
        .select(&selector)
        .filter_map(|el| el.value().attr("href").map(String::from));

    let selector = Selector::parse(".nidb-album p > strong")
        .expect("Coud not create '.nidb-album p > strong' selector");
    let names = doc.select(&selector).map(|el| el.inner_html());

    Ok(urls
        .zip(names)
        .map(|(url, name)| Link { url, name })
        .collect())
}

fn get_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();

    // pretend we're a browser
    headers.insert(
        HeaderName::from_static("user-agent"),
        HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36"));

    headers
}
