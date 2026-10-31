// SPDX-License-Identifier: AGPL-3.0-or-later

//! The targets against real servers, when they are given: every call, then
//! a repository's whole life (snapshots, an incremental run, forget, prune,
//! a full check). Run with a server's details in the environment:
//!
//! - `REPO_TEST_WEBDAV=url,user,password`
//! - `REPO_TEST_S3=endpoint,region,bucket,access key,secret key[,path]`
//!
//! Without them the tests pass without doing anything: the servers are
//! started separately (rclone serve, for example).

use std::sync::Arc;

use app_repo::testing::{repository_life, target_calls, unique};
use app_repo::{Error, S3Config, S3Target, Target, WebdavConfig, WebdavTarget};

fn parts(var: &str) -> Option<Vec<String>> {
    std::env::var(var)
        .ok()
        .map(|v| v.split(',').map(str::to_owned).collect())
}

#[test]
fn webdav_against_a_real_server() {
    let Some(p) = parts("REPO_TEST_WEBDAV") else {
        return;
    };
    let config = |url: String| WebdavConfig {
        url,
        username: p[1].clone(),
        password: p[2].clone(),
    };
    let root = WebdavTarget::new(&config(p[0].clone())).unwrap();
    target_calls(&root);
    // a repository in a folder that does not exist yet, with a space
    let folder = format!("{}/{} backups", p[0].trim_end_matches('/'), unique());
    let target: Arc<dyn Target> = Arc::new(WebdavTarget::new(&config(folder.clone())).unwrap());
    repository_life(&target);
    let wrong = WebdavTarget::new(&WebdavConfig {
        password: "wrong".into(),
        ..config(folder)
    })
    .unwrap();
    assert!(matches!(wrong.list(""), Err(Error::Target(m)) if m.contains("401")));
}

#[test]
fn s3_against_a_real_server() {
    let Some(p) = parts("REPO_TEST_S3") else {
        return;
    };
    let config = S3Config {
        endpoint: p[0].clone(),
        region: p[1].clone(),
        bucket: p[2].clone(),
        access_key: p[3].clone(),
        secret_key: p[4].clone(),
        path_style: p.get(5).is_some_and(|s| s == "path"),
        prefix: String::new(),
    };
    target_calls(&S3Target::new(config.clone()).unwrap());
    let target: Arc<dyn Target> = Arc::new(
        S3Target::new(S3Config {
            prefix: format!("{} backups/laptop", unique()),
            ..config.clone()
        })
        .unwrap(),
    );
    repository_life(&target);
    let wrong = S3Target::new(S3Config {
        secret_key: "wrong".into(),
        ..config
    })
    .unwrap();
    assert!(matches!(wrong.list(""), Err(Error::Target(m)) if m.contains("403")));
}

/// Through a proxy that cuts connections: `REPO_TEST_CUT=1` with the
/// servers above.
#[test]
fn webdav_and_s3_through_connections_that_break() {
    if std::env::var("REPO_TEST_CUT").is_err() {
        return;
    }
    let addr = |url: &str| -> std::net::SocketAddr {
        let host = url.split("://").nth(1).unwrap().split('/').next().unwrap();
        host.parse().unwrap()
    };
    if let Some(p) = parts("REPO_TEST_WEBDAV") {
        let folder = format!("{} cut", unique());
        let proxy = app_repo::testing::cutting_proxy(addr(&p[0]), 35, 1);
        let config = |base: String| WebdavConfig {
            url: format!("{}/{folder}", base.trim_end_matches('/')),
            username: p[1].clone(),
            password: p[2].clone(),
        };
        let cut: Arc<dyn Target> =
            Arc::new(WebdavTarget::new(&config(format!("http://{proxy}/"))).unwrap());
        let direct: Arc<dyn Target> = Arc::new(WebdavTarget::new(&config(p[0].clone())).unwrap());
        app_repo::testing::survives_cuts(&cut, &direct);
    }
    if let Some(p) = parts("REPO_TEST_S3") {
        let proxy = app_repo::testing::cutting_proxy(addr(&p[0]), 35, 2);
        let config = |endpoint: String| S3Config {
            endpoint,
            region: p[1].clone(),
            bucket: p[2].clone(),
            access_key: p[3].clone(),
            secret_key: p[4].clone(),
            path_style: true,
            prefix: format!("{} cut", unique()),
        };
        let prefix = config(String::new()).prefix;
        let cut: Arc<dyn Target> = Arc::new(
            S3Target::new(S3Config {
                prefix: prefix.clone(),
                ..config(format!("http://{proxy}"))
            })
            .unwrap(),
        );
        let direct: Arc<dyn Target> = Arc::new(
            S3Target::new(S3Config {
                prefix,
                ..config(p[0].clone())
            })
            .unwrap(),
        );
        app_repo::testing::survives_cuts(&cut, &direct);
    }
}
