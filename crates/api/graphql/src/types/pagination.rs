use async_graphql::SimpleObject;

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct PageInfo {
    pub has_next_page: bool,
    pub has_previous_page: bool,
    pub start_cursor: Option<String>,
    pub end_cursor: Option<String>,
}

#[derive(async_graphql::InputObject, Default)]
pub struct PaginationInput {
    pub first: Option<i32>,
    pub after: Option<String>,
    pub last: Option<i32>,
    pub before: Option<String>,
}

/// Generic cursor helper reused across paginated connections.
pub fn encode_cursor(offset: i64) -> String {
    base64_offset(offset)
}

pub fn decode_cursor(cursor: &str) -> Option<i64> {
    offset_from_base64(cursor)
}

fn base64_offset(n: i64) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(n.to_le_bytes())
}

fn offset_from_base64(s: &str) -> Option<i64> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()?;
    let arr: [u8; 8] = bytes.try_into().ok()?;
    Some(i64::from_le_bytes(arr))
}