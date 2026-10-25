// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use oxc::allocator::Allocator;
use oxc::ast::ast::CommentKind;
use oxc::parser::{ParseOptions, Parser};

use crate::MediaType;
use crate::parsing::get_source_type;

#[derive(Debug, Clone)]
pub enum TokenOrComment {
  Token(oxc::parser::Token),
  Comment { kind: CommentKind, text: String },
}

#[derive(Debug, Clone)]
pub struct LexedItem {
  /// Range of the token or comment.
  pub range: std::ops::Range<usize>,
  /// Token or comment.
  pub inner: TokenOrComment,
}

/// Given the source text and media type, tokenizes the provided
/// text to a collection of tokens and comments.
pub fn lex(source: &str, media_type: MediaType) -> Vec<LexedItem> {
  let allocator = Allocator::default();
  let source_type = get_source_type(media_type);

  // the lexer skips a leading BOM, but the ranges are still relative to the
  // original text
  let (bom_len, text) = match source.strip_prefix('\u{FEFF}') {
    Some(text) => ('\u{FEFF}'.len_utf8(), text),
    None => (0, source),
  };

  let ret = Parser::new(&allocator, text, source_type)
    .with_options(ParseOptions {
      parse_regular_expression: false,
      ..Default::default()
    })
    // tokens are only collected when the parser is configured to do so
    .with_config(oxc::parser::config::TokensParserConfig)
    .parse();

  let mut items: Vec<LexedItem> = Vec::new();

  // Extract comments from the parsed program
  for comment in &ret.program.comments {
    let start = comment.span.start as usize;
    let end = comment.span.end as usize;
    items.push(LexedItem {
      range: start + bom_len..end + bom_len,
      inner: TokenOrComment::Comment {
        kind: comment.kind,
        text: text[start..end].to_string(),
      },
    });
  }

  for &token in &ret.tokens {
    let start = token.start() as usize;
    let end = token.end() as usize;
    items.push(LexedItem {
      range: start + bom_len..end + bom_len,
      inner: TokenOrComment::Token(token),
    });
  }

  items.sort_by_key(|item| item.range.start);
  items
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::MediaType;
  use oxc::parser::Kind;

  #[test]
  fn tokenize_with_comments() {
    let items = lex(
      "const /* 1 */ t: number /* 2 */ = 5; // 3",
      MediaType::TypeScript,
    );
    assert_eq!(items.len(), 10);

    // only bother testing a few
    assert!(matches!(items[1].inner, TokenOrComment::Comment { .. }));
    assert!(matches!(
      items[3].inner,
      TokenOrComment::Token(token) if token.kind() == Kind::Colon
    ));
    assert!(matches!(items[9].inner, TokenOrComment::Comment { .. }));
  }

  #[test]
  fn handle_bom() {
    const BOM_CHAR: char = '\u{FEFF}';
    let items = lex(&format!("{}1", BOM_CHAR), MediaType::JavaScript);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].range.start, BOM_CHAR.len_utf8());
  }
}
