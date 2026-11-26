use regex::Regex;
use std::sync::OnceLock;

/// Bloco estruturado para renderização nativa em Slint sem exibir sintaxe Markdown/HTML crua.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RichBlock {
    Heading(String),
    Subheading(String),
    Paragraph(String),
    Bullet(String),
    Link {
        label: String,
        url: String,
    },
    Image {
        image_url: String,
        link_url: Option<String>,
        alt: String,
    },
    Separator,
}

/// Converte Markdown e HTML permitido (de descrições do Modrinth/CurseForge) em blocos seguros,
/// sanitizando scripts e tags desconhecidas e extraindo imagens, banners e links clicáveis.
pub fn parse_rich_description(input: &str) -> Vec<RichBlock> {
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    static LINKED_HTML_IMG_RE: OnceLock<Regex> = OnceLock::new();
    static LINKED_MD_IMG_RE: OnceLock<Regex> = OnceLock::new();
    static HTML_IMG_RE: OnceLock<Regex> = OnceLock::new();
    static MD_IMG_RE: OnceLock<Regex> = OnceLock::new();
    static MD_LINK_RE: OnceLock<Regex> = OnceLock::new();
    static HTML_A_RE: OnceLock<Regex> = OnceLock::new();
    static HTML_TAG_RE: OnceLock<Regex> = OnceLock::new();

    let script_re = SCRIPT_RE
        .get_or_init(|| Regex::new(r"(?is)<(script|style)[^>]*>.*?</(script|style)>").unwrap());
    let linked_html_img_re = LINKED_HTML_IMG_RE.get_or_init(|| {
        Regex::new(r#"(?i)\[\s*<img[^>]*src=["']([^"']+)["'][^>]*>\s*\]\((https?://[^)\s]+)\)"#)
            .unwrap()
    });
    let linked_md_img_re = LINKED_MD_IMG_RE.get_or_init(|| {
        Regex::new(r#"\[\s*!\[([^\]]*)\]\((https?://[^)\s]+)\)\s*\]\((https?://[^)\s]+)\)"#)
            .unwrap()
    });
    let html_img_re = HTML_IMG_RE
        .get_or_init(|| Regex::new(r#"(?i)<img[^>]*src=["']([^"']+)["'][^>]*>"#).unwrap());
    let md_img_re =
        MD_IMG_RE.get_or_init(|| Regex::new(r#"!\[([^\]]*)\]\((https?://[^)\s]+)\)"#).unwrap());
    let md_link_re =
        MD_LINK_RE.get_or_init(|| Regex::new(r#"\[([^\]]+)\]\((https?://[^)\s]+)\)"#).unwrap());
    let html_a_re = HTML_A_RE.get_or_init(|| {
        Regex::new(r#"(?is)<a[^>]*href=["'](https?://[^"']+)["'][^>]*>(.*?)</a>"#).unwrap()
    });
    let html_tag_re = HTML_TAG_RE.get_or_init(|| Regex::new(r"(?s)<[^>]+>").unwrap());

    let sanitized = script_re.replace_all(input, "");
    let mut blocks = Vec::new();

    for raw_line in sanitized.lines() {
        let mut line = raw_line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line == "---"
            || line == "***"
            || line == "___"
            || line.eq_ignore_ascii_case("<hr>")
            || line.eq_ignore_ascii_case("<hr/>")
        {
            blocks.push(RichBlock::Separator);
            continue;
        }

        // 1. Extrai imagens com link em formato `[<img src="..."/>](url)` (ex.: Cloth Config API)
        let mut extracted_images = false;
        for cap in linked_html_img_re.captures_iter(&line) {
            blocks.push(RichBlock::Image {
                image_url: cap[1].to_string(),
                link_url: Some(cap[2].to_string()),
                alt: String::new(),
            });
            extracted_images = true;
        }
        if extracted_images {
            line = linked_html_img_re.replace_all(&line, "").trim().to_string();
        }

        // 2. Extrai imagens com link em formato Markdown `[![alt](img)](url)`
        let mut extracted_md_linked = false;
        for cap in linked_md_img_re.captures_iter(&line) {
            blocks.push(RichBlock::Image {
                image_url: cap[2].to_string(),
                link_url: Some(cap[3].to_string()),
                alt: cap[1].to_string(),
            });
            extracted_md_linked = true;
        }
        if extracted_md_linked {
            line = linked_md_img_re.replace_all(&line, "").trim().to_string();
        }

        // 3. Extrai imagens HTML `<img src="..."/>` e Markdown `![alt](img)`
        for cap in html_img_re.captures_iter(&line) {
            blocks.push(RichBlock::Image {
                image_url: cap[1].to_string(),
                link_url: None,
                alt: String::new(),
            });
        }
        line = html_img_re.replace_all(&line, "").trim().to_string();

        for cap in md_img_re.captures_iter(&line) {
            blocks.push(RichBlock::Image {
                image_url: cap[2].to_string(),
                link_url: None,
                alt: cap[1].to_string(),
            });
        }
        line = md_img_re.replace_all(&line, "").trim().to_string();

        if line.is_empty() {
            continue;
        }

        // 4. Extrai links clicáveis (`<a href="...">` e `[label](url)`)
        let mut line_links: Vec<(String, String)> = Vec::new();
        for cap in html_a_re.captures_iter(&line) {
            let url = cap[1].to_string();
            let label = strip_inline_markup(&html_tag_re.replace_all(&cap[2], ""));
            if !label.is_empty() {
                line_links.push((label, url));
            }
        }
        line = html_a_re
            .replace_all(&line, |caps: &regex::Captures| caps[2].to_string())
            .to_string();

        for cap in md_link_re.captures_iter(&line) {
            let label = strip_inline_markup(&cap[1]);
            let url = cap[2].to_string();
            if !label.is_empty() {
                line_links.push((label, url));
            }
        }
        line = md_link_re
            .replace_all(&line, |caps: &regex::Captures| caps[1].to_string())
            .to_string();

        // Remove quaisquer tags HTML restantes
        line = html_tag_re.replace_all(&line, "").trim().to_string();
        let clean = strip_inline_markup(&line);
        if clean.is_empty() {
            for (label, url) in line_links {
                blocks.push(RichBlock::Link { label, url });
            }
            continue;
        }

        // Se a linha inteira era apenas um link (ex.: `[https://...](https://...)`), emite só o Link clicável
        if line_links.len() == 1 && clean == line_links[0].0 {
            let (label, url) = line_links.remove(0);
            blocks.push(RichBlock::Link { label, url });
            continue;
        }

        if let Some(h) = clean.strip_prefix("### ") {
            blocks.push(RichBlock::Subheading(h.trim().to_string()));
        } else if let Some(h) = clean.strip_prefix("## ") {
            blocks.push(RichBlock::Heading(h.trim().to_string()));
        } else if let Some(h) = clean.strip_prefix("# ") {
            blocks.push(RichBlock::Heading(h.trim().to_string()));
        } else if let Some(b) = clean
            .strip_prefix("- ")
            .or_else(|| clean.strip_prefix("* "))
        {
            blocks.push(RichBlock::Bullet(b.trim().to_string()));
        } else {
            blocks.push(RichBlock::Paragraph(clean));
        }

        for (label, url) in line_links {
            blocks.push(RichBlock::Link { label, url });
        }
    }

    blocks
}

fn strip_inline_markup(s: &str) -> String {
    s.replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace('`', "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .trim()
        .to_string()
}
