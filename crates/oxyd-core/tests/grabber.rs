//! The site grabber against pages served by the local test server.

use std::net::SocketAddr;

use oxyd_core::grabber::GrabOptions;
use oxyd_core::store::Store;
use oxyd_core::{Category, Manager};

#[tokio::test]
async fn collects_files_and_follows_pages() {
    let addr = test_server::spawn(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let base = format!("http://{addr}/blob");
    test_server::put_blob(
        "grab-index.html",
        br#"<html><body>
            <a href="one.zip">one</a> <a href="one.zip#again">same</a>
            <img src="/blob/pic.png">
            <a href="grab-more.html">more</a>
            <a href="http://elsewhere.invalid/page.html">other site</a>
            <a href="https://cdn.example.org/movie.mkv">cdn</a>
        </body></html>"#
            .to_vec(),
    );
    test_server::put_blob(
        "grab-more.html",
        br#"<a href="two.pdf">two</a> <a href="grab-index.html">back</a> <a href="missing.html">x</a>"#.to_vec(),
    );
    let m = Manager::with_store(Store::open_in_memory().unwrap()).unwrap();
    let start = format!("{base}/grab-index.html");

    let shallow = m.grab(&start, GrabOptions::default()).await.unwrap();
    let names: Vec<&str> = shallow.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["one.zip", "pic.png", "movie.mkv"]);
    assert_eq!(shallow.items[1].category, Category::Image);
    assert_eq!(shallow.items[0].url, format!("{base}/one.zip"));
    assert_eq!(shallow.items[0].page, start);
    assert_eq!((shallow.pages, shallow.skipped), (1, 0));

    let deep = m
        .grab(
            &start,
            GrabOptions {
                depth: 1,
                same_site: true,
            },
        )
        .await
        .unwrap();
    let names: Vec<&str> = deep.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["one.zip", "pic.png", "movie.mkv", "two.pdf"]);
    // The other site was not visited, the page was not read twice.
    assert_eq!((deep.pages, deep.skipped), (2, 0));

    // A file is not a page.
    assert!(m
        .grab(&format!("{base}/one.zip"), GrabOptions::default())
        .await
        .is_err());
}
