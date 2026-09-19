use eframe::egui::{self, RichText, Ui};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

pub fn show(ui: &mut Ui, markdown: &str) {
    let mut paragraph = String::new();
    let mut heading: Option<HeadingLevel> = None;
    let mut list_depth: usize = 0;
    let mut is_code_block = false;
    let mut code = String::new();

    let flush_para = |ui: &mut Ui, paragraph: &mut String, heading: &mut Option<HeadingLevel>, list_depth: usize| {
        let text = paragraph.trim_end().to_string();
        paragraph.clear();
        if text.is_empty() {
            return;
        }
        let prefix = if list_depth > 0 {
            format!("{}• ", "  ".repeat(list_depth.saturating_sub(1)))
        } else {
            String::new()
        };
        match heading.take() {
            Some(HeadingLevel::H1) => {
                ui.add(egui::Label::new(RichText::new(format!("{prefix}{text}")).heading()).wrap());
            }
            Some(HeadingLevel::H2) | Some(HeadingLevel::H3) => {
                ui.add(
                    egui::Label::new(RichText::new(format!("{prefix}{text}")).strong().size(16.0))
                        .wrap(),
                );
            }
            Some(_) => {
                ui.add(egui::Label::new(RichText::new(format!("{prefix}{text}")).strong()).wrap());
            }
            None => {
                ui.add(egui::Label::new(format!("{prefix}{text}")).wrap());
            }
        }
    };

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
                heading = Some(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
            }
            Event::Start(Tag::List(_)) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
                list_depth = list_depth.saturating_sub(1);
            }
            Event::Start(Tag::Item) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
            }
            Event::End(TagEnd::Item) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
                is_code_block = true;
                code.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                is_code_block = false;
                ui.add(
                    egui::Label::new(RichText::new(code.trim_end()).monospace()).wrap(),
                );
                code.clear();
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
            }
            Event::Text(t) | Event::Code(t) => {
                if is_code_block {
                    code.push_str(&t);
                } else {
                    paragraph.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if is_code_block {
                    code.push('\n');
                } else {
                    paragraph.push('\n');
                }
            }
            Event::Rule => {
                flush_para(ui, &mut paragraph, &mut heading, list_depth);
                ui.separator();
            }
            _ => {}
        }
    }
    flush_para(ui, &mut paragraph, &mut heading, list_depth);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_headings_and_lists() {
        let md = "## 背景\n\n- item\n\n`code`";
        let n = Parser::new(md).count();
        assert!(n > 3);
    }
}
