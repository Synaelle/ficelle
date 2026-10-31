// SPDX-License-Identifier: AGPL-3.0-or-later

use std::net::IpAddr;

use app_protocol::feedback::{Category, Report, solve};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rusqlite::Connection;

use super::*;

fn conn() -> Connection {
    crate::db::feedback_connection()
}

fn config() -> FeedbackConfig {
    FeedbackConfig {
        pow_bits: 8,
        ..FeedbackConfig::default()
    }
}

fn report(message: &str) -> Report {
    Report {
        challenge: String::new(),
        nonce: 0,
        install: Some("0123456789abcdef0123456789abcdef".into()),
        category: Some(Category::Bug),
        about: None,
        message: message.into(),
        contact: None,
        version: "0.12.0-nightly.3".into(),
        platform: "windows".into(),
        logs: None,
        images: Vec::new(),
        server: None,
        receipt: None,
    }
}

const NOW: i64 = 1_800_000_000_000;

#[test]
fn a_challenge_is_answered_once_and_only_while_it_lasts() {
    let c = conn();
    let address = address_key(&c, "10.0.0.1".parse().unwrap()).unwrap();
    let challenge = super::challenge(&c, NOW, &address, &config()).unwrap();
    assert_eq!(challenge.bits, 8);
    let nonce = solve(&challenge.challenge, challenge.bits);

    let wrong =
        (0_u64..u64::MAX).find(|n| !app_protocol::feedback::meets(&challenge.challenge, *n, 8));
    assert_eq!(
        take_challenge(&c, NOW, &challenge.challenge, wrong.unwrap()).unwrap(),
        Err(ChallengeRefused::NotSolved)
    );
    assert_eq!(
        take_challenge(&c, NOW + CHALLENGE_LIFE, &challenge.challenge, nonce).unwrap(),
        Err(ChallengeRefused::Expired)
    );
    assert_eq!(
        take_challenge(&c, NOW, &challenge.challenge, nonce).unwrap(),
        Ok(())
    );
    assert_eq!(
        take_challenge(&c, NOW, &challenge.challenge, nonce).unwrap(),
        Err(ChallengeRefused::Used)
    );
}

#[test]
fn a_forged_or_changed_challenge_is_not_ours() {
    let c = conn();
    let challenge = super::challenge(&c, NOW, "a", &config()).unwrap();
    let mut token = URL_SAFE_NO_PAD.decode(&challenge.challenge).unwrap();
    // asking less work of itself: the seal no longer matches
    token[24] = 0;
    let easier = URL_SAFE_NO_PAD.encode(&token);
    assert_eq!(
        take_challenge(&c, NOW, &easier, 0).unwrap(),
        Err(ChallengeRefused::Unknown)
    );
    for junk in ["", "!!!", "AAAA", &"A".repeat(55)] {
        assert_eq!(
            take_challenge(&c, NOW, junk, 0).unwrap(),
            Err(ChallengeRefused::Unknown),
            "{junk}"
        );
    }
    // another server's challenge, its own secret
    let other = conn();
    let theirs = super::challenge(&other, NOW, "a", &config()).unwrap();
    let nonce = solve(&theirs.challenge, theirs.bits);
    assert_eq!(
        take_challenge(&c, NOW, &theirs.challenge, nonce).unwrap(),
        Err(ChallengeRefused::Unknown)
    );
}

#[test]
fn busy_senders_get_harder_challenges_up_to_a_ceiling() {
    let mut c = conn();
    let config = FeedbackConfig {
        pow_bits: 18,
        per_hour: 100,
        per_day: 100,
        ..FeedbackConfig::default()
    };
    let address = "busy";
    let mut seen = Vec::new();
    for n in 0..5 {
        seen.push(
            super::challenge(&c, NOW + n, address, &config)
                .unwrap()
                .bits,
        );
        let incoming = validate(report(&format!("report number {n} about a crash"))).unwrap();
        take(&mut c, NOW + n, address, &incoming).unwrap();
    }
    assert_eq!(seen, [18, 20, 22, 24, 24]);
    assert_eq!(super::challenge(&c, NOW, "calm", &config).unwrap().bits, 18);
    let later = NOW + 61 * MINUTE;
    assert_eq!(
        super::challenge(&c, later, address, &config).unwrap().bits,
        18
    );
}

#[test]
fn addresses_are_hashed_and_ipv6_counts_by_its_first_half() {
    let c = conn();
    let key = |ip: &str| address_key(&c, ip.parse::<IpAddr>().unwrap()).unwrap();
    let one = key("10.0.0.1");
    assert_eq!(one.len(), 32);
    assert!(!one.contains("10.0.0.1"));
    assert_ne!(one, key("10.0.0.2"));
    assert_eq!(one, key("::ffff:10.0.0.1"), "mapped is the same address");
    assert_eq!(key("2001:db8:1:2::1"), key("2001:db8:1:2:ffff::9"));
    assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
    assert_ne!(
        one,
        address_key(&conn(), "10.0.0.1".parse().unwrap()).unwrap()
    );
}

#[test]
fn limits_per_address_install_and_instance() {
    let mut c = conn();
    let config = FeedbackConfig {
        per_hour: 3,
        per_day: 5,
        instance_per_day: 8,
        ..config()
    };
    let install = "0123456789abcdef0123456789abcdef";
    let send = |c: &mut Connection, at: i64, address: &str, install: Option<&str>| {
        let mut r = report(&format!("a report at {at} from {address}"));
        r.install = install.map(Into::into);
        take(c, at, address, &validate(r).unwrap()).unwrap();
    };
    for n in 0..3 {
        assert_eq!(
            check_limits(&c, NOW, "a", Some(install), &config).unwrap(),
            Ok(())
        );
        send(&mut c, NOW + n, "a", Some(install));
    }
    let wait = check_limits(&c, NOW + 10, "a", Some(install), &config)
        .unwrap()
        .unwrap_err();
    assert!((3590..=3600).contains(&wait), "{wait}");
    // the same install from another address is still the same install
    assert!(
        check_limits(&c, NOW + 10, "b", Some(install), &config)
            .unwrap()
            .is_err()
    );
    // the address with another install is still the same address
    assert!(
        check_limits(&c, NOW + 10, "a", None, &config)
            .unwrap()
            .is_err()
    );
    // an hour on, the day's limit is two away
    let later = NOW + HOUR + 10;
    send(&mut c, later, "a", Some(install));
    send(&mut c, later + 1, "a", Some(install));
    assert!(
        check_limits(&c, later + 2, "a", Some(install), &config)
            .unwrap()
            .is_err()
    );

    // without an install id, half the address's allowance
    for n in 0..1 {
        assert_eq!(check_limits(&c, NOW, "c", None, &config).unwrap(), Ok(()));
        send(&mut c, NOW + n, "c", None);
    }
    assert!(
        check_limits(&c, NOW + 5, "c", None, &config)
            .unwrap()
            .is_err()
    );

    // the whole instance: 6 today, 2 more and nobody sends until tomorrow
    let today = NOW - NOW.rem_euclid(DAY) + HOUR * 2;
    let mut d = conn();
    for n in 0..8 {
        send(&mut d, today + n, &format!("x{n}"), None);
    }
    let wait = check_limits(&d, today + 10, "new", None, &config)
        .unwrap()
        .unwrap_err();
    assert_eq!(
        wait,
        u64::try_from((DAY - 2 * HOUR - 10) / 1000 + 1).unwrap()
    );
    assert!(
        check_limits(&d, today + DAY, "new", None, &config)
            .unwrap()
            .is_ok()
    );
}

#[test]
fn forms_and_sizes_are_checked() {
    let fine = validate(report("  The editor froze when I pasted a table.  ")).unwrap();
    assert_eq!(fine.message, "The editor froze when I pasted a table.");

    let refused = |change: &dyn Fn(&mut Report)| {
        let mut r = report("the editor froze");
        change(&mut r);
        validate(r).unwrap_err()
    };
    assert!(refused(&|r| r.message = "   ".into()).contains("empty"));
    assert!(refused(&|r| r.message = "x".repeat(5_001)).contains("5000"));
    assert!(refused(&|r| r.message = "bell\u{7}".into()).contains("control"));
    assert!(refused(&|r| r.version = "1.0<script>".into()).contains("version"));
    assert!(refused(&|r| r.version = String::new()).contains("version"));
    assert!(refused(&|r| r.platform = "Windows 11".into()).contains("platform"));
    for bad in [
        "a@b",
        "a@b.c@d.e",
        "@a@b.org",
        "x",
        "two words",
        "name!",
        "n#12",
        &"a".repeat(33),
    ] {
        assert!(
            refused(&|r| r.contact = Some(bad.into())).contains("contact"),
            "{bad}"
        );
    }
    assert!(refused(&|r| r.about = Some("x".repeat(121))).contains("120"));
    assert!(
        refused(&|r| r.about = Some(
            "two
lines"
                .into()
        ))
        .contains("control")
    );
    let about_only = |r: &mut Report| {
        r.message = "  ".into();
        r.about = Some(" Sync stops after lock ".into());
    };
    let mut r = report("");
    about_only(&mut r);
    let taken = validate(r).unwrap();
    assert_eq!(taken.about.as_deref(), Some("Sync stops after lock"));
    assert_eq!(taken.message, "");
    for (given, kept) in [
        ("@Synaelle", "synaelle"),
        ("kim.dev_2", "kim.dev_2"),
        ("Old Name#0042", ""),
        ("OldName#0042", "oldname#0042"),
    ] {
        let mut r = report("the editor froze");
        r.contact = Some(given.into());
        match validate(r) {
            Ok(taken) => assert_eq!(taken.contact.as_deref(), Some(kept), "{given}"),
            Err(_) => assert_eq!(kept, "", "{given}"),
        }
    }
    assert!(refused(&|r| r.logs = Some("x".repeat(64 * 1024 + 1))).contains("64 KB"));
    assert!(refused(&|r| r.install = Some("ABC".into())).contains("install"));
    let png = STANDARD.encode(images::tests::png_of(10, 10));
    assert!(refused(&|r| r.images = vec![png.clone(); 4]).contains("more than 3"));
    assert!(refused(&|r| r.images = vec!["not base64!".into()]).contains("base64"));
    let gif = STANDARD.encode(b"GIF89a.......");
    assert!(refused(&|r| r.images = vec![gif.clone()]).contains("image 1"));
    let huge = "A".repeat(3 * 1024 * 1024);
    assert!(refused(&|r| r.images = vec![huge.clone()]).contains("2 MB"));

    let mut r = report("the editor froze");
    r.contact = Some(" Kim@Example.ORG ".into());
    r.logs = Some("  ".into());
    r.images = vec![png.clone(), png];
    let taken = validate(r).unwrap();
    assert_eq!(taken.contact.as_deref(), Some("kim@example.org"));
    assert_eq!(taken.logs, None);
    assert_eq!(taken.images.len(), 2);
}

#[test]
fn spam_is_filed_away_and_blocked_senders_dropped() {
    let mut c = conn();
    let mut send = |at: i64, address: &str, r: Report| {
        take(&mut c, at, address, &validate(r).unwrap()).unwrap()
    };
    let words = "The sidebar does not open after the last update on my phone.";
    assert!(matches!(send(NOW, "a", report(words)), Outcome::Saved(_)));
    // the same words again, whoever sends them
    assert!(matches!(
        send(NOW + 1, "b", report(words)),
        Outcome::Spam(_)
    ));
    let ads = "cheap pills https://x.example https://y.example www.z.example";
    assert!(matches!(send(NOW + 2, "c", report(ads)), Outcome::Spam(_)));
    assert!(matches!(
        send(NOW + 3, "d", report("$$$###!!")),
        Outcome::Spam(_)
    ));
    assert!(matches!(
        send(NOW + 4, "e", report("crash!!!")),
        Outcome::Saved(_)
    ));
    assert!(matches!(
        send(
            NOW + 5,
            "e",
            report("one link https://github.com/x/y/issues/1 helps")
        ),
        Outcome::Saved(_)
    ));
    drop(send);

    block(&c, NOW, BlockKind::Word, "Casino", "", None).unwrap();
    let mut send = |at: i64, address: &str, r: Report| {
        take(&mut c, at, address, &validate(r).unwrap()).unwrap()
    };
    assert!(matches!(
        send(NOW + 6, "f", report("best casino bonus for your notes app")),
        Outcome::Spam(_)
    ));
    drop(send);

    block(&c, NOW, BlockKind::Address, "g", "flood", Some(NOW + DAY)).unwrap();
    block(&c, NOW, BlockKind::Email, " Spam@Example.org", "", None).unwrap();
    block(
        &c,
        NOW,
        BlockKind::Install,
        "ffffffffffffffffffffffffffffffff",
        "",
        None,
    )
    .unwrap();
    let mut send = |at: i64, address: &str, r: Report| {
        take(&mut c, at, address, &validate(r).unwrap()).unwrap()
    };
    assert_eq!(
        send(NOW + 7, "g", report("a fine report about sync")),
        Outcome::Dropped
    );
    let mut by_mail = report("another fine report about sync");
    by_mail.contact = Some("spam@example.ORG".into());
    assert_eq!(send(NOW + 8, "h", by_mail), Outcome::Dropped);
    let mut by_install = report("a third fine report about sync");
    by_install.install = Some("f".repeat(32));
    assert_eq!(send(NOW + 9, "h", by_install), Outcome::Dropped);
    // the address block ends
    assert!(matches!(
        send(
            NOW + DAY + 1,
            "g",
            report("back after a day, a fine report")
        ),
        Outcome::Saved(_)
    ));
    drop(send);

    let (kept, sends): (i64, i64) = c
        .query_row(
            "SELECT (SELECT count(*) FROM reports), (SELECT count(*) FROM sends)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (kept, sends),
        (8, 11),
        "dropped ones are not kept but counted"
    );
    let flags: String = c
        .query_row(
            "SELECT flags FROM reports WHERE message LIKE 'cheap pills%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(flags, "links");
}

#[test]
fn reports_and_their_images_go_after_the_keeping_time_unless_kept() {
    let mut c = conn();
    let mut r = report("an old report with a picture");
    r.images = vec![STANDARD.encode(images::tests::png_of(4, 4))];
    let Outcome::Saved(old) = take(&mut c, NOW, "a", &validate(r).unwrap()).unwrap() else {
        panic!("saved")
    };
    let Outcome::Saved(kept) = take(
        &mut c,
        NOW,
        "a",
        &validate(report("an old one worth keeping")).unwrap(),
    )
    .unwrap() else {
        panic!("saved")
    };
    c.execute("UPDATE reports SET kept = 1 WHERE id = ?1", [&kept])
        .unwrap();
    super::challenge(&c, NOW, "a", &config()).unwrap();
    let later = NOW + 181 * DAY;
    take(
        &mut c,
        later,
        "b",
        &validate(report("a new report, still here")).unwrap(),
    )
    .unwrap();
    assert!(cleanup(&c, later, 180).unwrap() >= 3);
    let ids: Vec<String> = c
        .prepare("SELECT id FROM reports ORDER BY created_at")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&kept) && !ids.contains(&old));
    let images: i64 = c
        .query_row("SELECT count(*) FROM images", [], |r| r.get(0))
        .unwrap();
    assert_eq!(images, 0, "its image went with it");
}

#[test]
fn the_apps_are_never_told_to_send_here_when_this_server_does_not_receive() {
    let off = Settings {
        receive: false,
        send_to: SendTo::Here,
    };
    assert_eq!(off.info().send_to, SendTo::Off);
    let on = Settings {
        receive: true,
        ..off
    };
    assert_eq!(on.info().send_to, SendTo::Here);
}
