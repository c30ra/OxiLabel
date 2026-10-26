//! The query types shared with the clients must survive the same URL parser the handlers use.

use axum::{extract::Query, http::Uri};
use oxilabel_core::{
    ImageFilter,
    api::{
        ArchiveExportQuery, BrowseQuery, DeleteLabelQuery, ImagePageQuery, NeighboursQuery,
        TileQuery, with_query,
    },
};
use serde::de::DeserializeOwned;

fn round_trip<T: DeserializeOwned>(query: &oxilabel_core::api::QueryPairs) -> T {
    let uri: Uri = with_query("/api/test", query).parse().expect("a valid URI");
    Query::<T>::try_from_uri(&uri)
        .expect("the server's query parser accepts the client's query")
        .0
}

#[test]
fn shared_queries_round_trip_through_the_server_parser() {
    let browse = BrowseQuery {
        path: "a folder/one+two".into(),
        project: Some(7),
    };
    assert_eq!(round_trip::<BrowseQuery>(&browse.to_query()), browse);

    let page = ImagePageQuery {
        offset: 40,
        limit: 20,
    };
    assert_eq!(round_trip::<ImagePageQuery>(&page.to_query()), page);

    let filter = ImageFilter {
        name: Some("a+b [0-9]".into()),
        min_width: Some(10),
        max_width: Some(100),
        min_height: Some(20),
        max_height: Some(200),
        min_size: Some(30),
        max_size: Some(300),
        modified_from: Some("2024-01-01".into()),
        modified_to: Some("2024-12-31".into()),
        job: Some(3),
        without_job: true,
        annotated: Some(false),
        same_name: true,
        labels: vec![2, 5],
    };
    assert_eq!(round_trip::<ImageFilter>(&filter.to_query()), filter);

    let neighbours = NeighboursQuery { count: 4 };
    assert_eq!(
        round_trip::<NeighboursQuery>(&neighbours.to_query()),
        neighbours
    );

    let tile = TileQuery {
        filter: Some("clahe_0.05".into()),
    };
    assert_eq!(round_trip::<TileQuery>(&tile.to_query()), tile);

    let delete = DeleteLabelQuery { confirm: true };
    assert_eq!(round_trip::<DeleteLabelQuery>(&delete.to_query()), delete);

    let archive = ArchiveExportQuery { images: true };
    assert_eq!(
        round_trip::<ArchiveExportQuery>(&archive.to_query()),
        archive
    );
}

#[test]
fn empty_queries_keep_the_server_defaults() {
    assert_eq!(
        round_trip::<BrowseQuery>(&BrowseQuery::default().to_query()),
        BrowseQuery::default()
    );
    assert_eq!(
        round_trip::<ImagePageQuery>(&ImagePageQuery::default().to_query()),
        ImagePageQuery::default()
    );
    assert_eq!(
        round_trip::<NeighboursQuery>(&NeighboursQuery::default().to_query()),
        NeighboursQuery::default()
    );
    assert_eq!(
        round_trip::<TileQuery>(&TileQuery::default().to_query()),
        TileQuery::default()
    );
    assert_eq!(
        round_trip::<DeleteLabelQuery>(&DeleteLabelQuery::default().to_query()),
        DeleteLabelQuery::default()
    );
    assert_eq!(
        round_trip::<ArchiveExportQuery>(&ArchiveExportQuery::default().to_query()),
        ArchiveExportQuery::default()
    );
}
