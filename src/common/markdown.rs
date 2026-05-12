use ammonia::clean;
use pulldown_cmark::{Parser, html};

pub fn render_markdown(input: &str) -> String {
    let parser = Parser::new(input);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    clean(&html_output)
}
