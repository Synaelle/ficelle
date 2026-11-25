// SPDX-License-Identifier: AGPL-3.0-or-later

//! The client and server halves together, as an account lives: sign-up,
//! sign-in, a wrong password, a new password, recovery.

use app_e2ee::server::Setup;
use app_e2ee::{
    AccountKeys, Error, Ksf, login_finish, login_start, new_account, registration_finish,
    registration_start,
};

fn register(setup: &Setup, username: &str, password: &str) -> (Vec<u8>, Vec<u8>) {
    register_with(setup, username, password, Ksf::CURRENT)
}

fn register_with(setup: &Setup, username: &str, password: &str, ksf: Ksf) -> (Vec<u8>, Vec<u8>) {
    let start = registration_start(password).unwrap();
    let response = setup
        .registration_response(&start.request, username)
        .unwrap();
    let finish = registration_finish(&start.state, password, &response, username, ksf).unwrap();
    (
        Setup::record(&finish.upload).unwrap(),
        finish.export_key.to_vec(),
    )
}

fn sign_in(
    setup: &Setup,
    record: Option<&[u8]>,
    username: &str,
    password: &str,
) -> Result<Vec<u8>, Error> {
    sign_in_with(setup, record, username, password, Ksf::CURRENT)
}

fn sign_in_with(
    setup: &Setup,
    record: Option<&[u8]>,
    username: &str,
    password: &str,
    ksf: Ksf,
) -> Result<Vec<u8>, Error> {
    let start = login_start(password).unwrap();
    let server = setup.login_start(record, &start.request, username).unwrap();
    let finish = login_finish(&start.state, password, &server.response, username, ksf)?;
    Setup::login_finish(&server.state, &finish.finalization, username)?;
    Ok(finish.export_key.to_vec())
}

#[test]
fn an_account_from_sign_up_to_recovery() {
    let setup = Setup::from_bytes(&Setup::generate().to_bytes()).unwrap();
    let (record, export_key) = register(&setup, "anna", "correct horse");
    let account = new_account(&export_key);

    // the same export key comes back at every sign-in and opens the keys
    let again = sign_in(&setup, Some(&record), " Anna", "correct horse").unwrap();
    assert_eq!(again, export_key);
    let master = AccountKeys::open_with_password(&account.bundle, &again).unwrap();
    assert_eq!(master.as_bytes(), account.master.as_bytes());

    // wrong password, another username, no account: all refused alike
    assert_eq!(
        sign_in(&setup, Some(&record), "anna", "wrong horse"),
        Err(Error::Refused)
    );
    assert_eq!(
        sign_in(&setup, Some(&record), "bob", "correct horse"),
        Err(Error::Refused)
    );
    assert_eq!(
        sign_in(&setup, None, "nobody", "correct horse"),
        Err(Error::Refused)
    );

    // a new password: new record, new wrapping, same master key
    let (record2, export2) = register(&setup, "anna", "battery staple");
    let bundle2 = AccountKeys::rewrap_password(&account.bundle, &master, &export2);
    let signed = sign_in(&setup, Some(&record2), "anna", "battery staple").unwrap();
    let master2 = AccountKeys::open_with_password(&bundle2, &signed).unwrap();
    assert_eq!(master2.as_bytes(), account.master.as_bytes());
    assert!(sign_in(&setup, Some(&record2), "anna", "correct horse").is_err());
    assert!(
        matches!(
            AccountKeys::open_with_password(&bundle2, &export_key),
            Err(Error::WrongKey)
        ),
        "the old password no longer opens the keys"
    );

    // the recovery code, typed loosely, opens the same key
    let typed = account.recovery_code.to_lowercase().replace('-', " ");
    let recovered = AccountKeys::open_with_recovery(&bundle2, &typed).unwrap();
    assert_eq!(recovered.as_bytes(), account.master.as_bytes());
    assert_eq!(
        AccountKeys::recovery_verifier(&typed).unwrap(),
        account.recovery_verifier
    );
    let other = new_account(&export_key);
    assert!(matches!(
        AccountKeys::open_with_recovery(&bundle2, &other.recovery_code),
        Err(Error::WrongKey)
    ));
}

#[test]
fn another_server_cannot_sign_in_with_a_record() {
    let setup = Setup::generate();
    let (record, _) = register(&setup, "anna", "correct horse");
    let impostor = Setup::generate();
    assert_eq!(
        sign_in(&impostor, Some(&record), "anna", "correct horse"),
        Err(Error::Refused)
    );
}

#[test]
fn an_old_record_is_strengthened_with_the_same_password() {
    let setup = Setup::generate();
    let (old, old_key) = register_with(&setup, "anna", "correct horse", Ksf::V1);
    let account = new_account(&old_key);

    // a record opens only with the stretching it was made with
    let signed = sign_in_with(&setup, Some(&old), "anna", "correct horse", Ksf::V1).unwrap();
    assert_eq!(signed, old_key);
    assert_eq!(
        sign_in_with(&setup, Some(&old), "anna", "correct horse", Ksf::V2),
        Err(Error::Refused)
    );

    // registered again: a new export key, the master key wrapped for it
    let master = AccountKeys::open_with_password(&account.bundle, &signed).unwrap();
    let (strong, strong_key) = register_with(&setup, "anna", "correct horse", Ksf::V2);
    assert_ne!(strong_key, old_key);
    let bundle = AccountKeys::rewrap_password(&account.bundle, &master, &strong_key);
    let again = sign_in_with(&setup, Some(&strong), "anna", "correct horse", Ksf::V2).unwrap();
    let opened = AccountKeys::open_with_password(&bundle, &again).unwrap();
    assert_eq!(opened.as_bytes(), account.master.as_bytes());
    assert_eq!(
        sign_in_with(&setup, Some(&strong), "anna", "correct horse", Ksf::V1),
        Err(Error::Refused)
    );
}

#[test]
fn stretching_numbers_round_trip() {
    for ksf in [Ksf::V1, Ksf::V2] {
        assert_eq!(Ksf::from_number(ksf.number()), Some(ksf));
    }
    assert_eq!(Ksf::from_number(0), None);
    assert_eq!(Ksf::from_number(3), None);
    assert_eq!(Ksf::CURRENT, Ksf::V2);
}

#[test]
fn the_backup_key_opens_with_the_recovery_code_of_its_account_only() {
    let setup = Setup::generate();
    let (_, export_key) = register(&setup, "anna", "correct horse");
    let account = new_account(&export_key);
    let made = AccountKeys::new_backup_key(&account.master);
    assert_eq!(made.key.len(), 43, "256 bits as base64url text");
    assert_ne!(
        *AccountKeys::new_backup_key(&account.master).key,
        *made.key,
        "each key is new"
    );

    // a fresh server has the bundle and the wrapped key, the person the code
    let master = AccountKeys::open_with_recovery(&account.bundle, &account.recovery_code).unwrap();
    let opened = AccountKeys::open_backup_key(&master, &made.wrapped);
    assert_eq!(opened.as_deref().map(String::as_str), Ok(made.key.as_str()));

    let (_, other_export) = register(&setup, "bob", "battery staple");
    let other = new_account(&other_export);
    assert_eq!(
        AccountKeys::open_backup_key(&other.master, &made.wrapped).map(|_| ()),
        Err(Error::WrongKey)
    );
    assert_eq!(
        AccountKeys::open_backup_key(&master, "not a key").map(|_| ()),
        Err(Error::Malformed("wrapped key"))
    );
    // a master key wrapping is not a backup key wrapping
    assert_eq!(
        AccountKeys::open_backup_key(&master, &account.bundle.by_recovery).map(|_| ()),
        Err(Error::WrongKey)
    );
}
