//! Dev helper: print an Argon2id hash for seeding a test user.
fn main() {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Secret123!".to_string());
    match talentos_authz::password::hash_password(&arg) {
        Ok(h) => println!("{h}"),
        Err(e) => eprintln!("hash failed: {e}"),
    }
}
