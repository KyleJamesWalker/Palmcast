use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/"]
#[exclude = "*.test.mjs"]
pub struct Web;
