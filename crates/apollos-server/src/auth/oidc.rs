#[derive(Debug, Clone)]
pub struct OidcIdentity {
    pub subject: String,
}

pub fn verify_opaque_token(token: &str) -> Option<OidcIdentity> {
    let trimmed = token.trim();
    if trimmed.len() < 16 {
        return None;
    }

    Some(OidcIdentity {
        subject: format!("oidc:{}", &trimmed[..12]),
    })
}
