//! Bounded native coloring of the managed Bash input line; shell bytes are unchanged.
use crate::event::GpuiEventProxy;
use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{Term, TermMode, cell::Flags},
    vte::ansi::Rgb,
};

const COMMAND: Rgb = Rgb {
    r: 124,
    g: 183,
    b: 255,
};
const STRING: Rgb = Rgb {
    r: 154,
    g: 207,
    b: 160,
};
const OPTION: Rgb = Rgb {
    r: 233,
    g: 196,
    b: 138,
};
const VARIABLE: Rgb = Rgb {
    r: 196,
    g: 165,
    b: 239,
};
const COMMENT: Rgb = Rgb {
    r: 140,
    g: 154,
    b: 176,
};
const ERROR: Rgb = Rgb {
    r: 240,
    g: 134,
    b: 145,
};

/// Lex colors indexed by Unicode scalar, without invoking a shell or reading PATH.
pub fn colors(text: &[char]) -> Vec<Option<Rgb>> {
    let mut result = vec![None; text.len()];
    let mut index = 0;
    let mut command = true;
    while index < text.len() {
        let c = text[index];
        if c.is_whitespace() {
            index += 1;
            continue;
        }
        if matches!(c, ';' | '|' | '&' | '(' | ')') {
            command = true;
            index += 1;
            continue;
        }
        if c == '#' {
            result[index..].fill(Some(COMMENT));
            break;
        }
        if matches!(c, '\'' | '"' | '`') {
            let start = index;
            let quote = c;
            index += 1;
            let mut closed = false;
            while index < text.len() {
                if text[index] == '\\' && quote != '\'' {
                    index = (index + 2).min(text.len());
                    continue;
                }
                if text[index] == quote {
                    index += 1;
                    closed = true;
                    break;
                }
                index += 1;
            }
            result[start..index].fill(Some(if closed { STRING } else { ERROR }));
            if quote == '"' {
                for n in start + 1..index {
                    if text[n] == '$' {
                        result[n] = Some(VARIABLE);
                    }
                }
            }
            command = false;
            continue;
        }
        let start = index;
        while index < text.len()
            && !text[index].is_whitespace()
            && !matches!(text[index], ';' | '|' | '&' | '(' | ')' | '\'' | '"' | '`')
        {
            if text[index] == '\\' {
                index = (index + 2).min(text.len());
            } else {
                index += 1;
            }
        }
        if index == start {
            index += 1;
            continue;
        }
        let assignment = text[start..index].contains(&'=') && command;
        let color = if c == '$' || assignment {
            Some(VARIABLE)
        } else if command {
            Some(COMMAND)
        } else if c == '-' {
            Some(OPTION)
        } else {
            None
        };
        result[start..index].fill(color);
        if !assignment {
            command = false;
        }
    }
    result
}

/// Map the visible managed prompt to cells, including wrapped and wide Unicode input.
pub fn input_colors(term: &Term<GpuiEventProxy>) -> Vec<(Point, Rgb)> {
    let grid = term.grid();
    if term.columns() < 2
        || grid.display_offset() != 0
        || term.mode().contains(TermMode::ALT_SCREEN)
    {
        return Vec::new();
    }
    let cursor = grid.cursor.point.line.0.max(0);
    let Some(start) = ((cursor - 31).max(0)..=cursor).rev().find(|line| {
        grid[Point::new(Line(*line), Column(0))].c == '❯'
            && grid[Point::new(Line(*line), Column(1))].c == ' '
    }) else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    for row in start..=cursor {
        for col in if row == start { 2 } else { 0 }..term.columns() {
            let point = Point::new(Line(row), Column(col));
            let cell = &grid[point];
            if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                cells.push((point, cell.c));
            }
        }
    }
    while cells
        .last()
        .is_some_and(|(_, c)| c.is_whitespace() || *c == '\0')
    {
        cells.pop();
    }
    let text: Vec<_> = cells.iter().map(|(_, c)| *c).collect();
    cells
        .into_iter()
        .zip(colors(&text))
        .filter_map(|((point, _), color)| color.map(|color| (point, color)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colors_commands_quotes_flags_variables_and_pipelines() {
        let text: Vec<_> = "git log --oneline \"中文🙂\" | grep $BRANCH # note"
            .chars()
            .collect();
        let colors = colors(&text);
        let at = |needle: &str| {
            let needle: Vec<_> = needle.chars().collect();
            text.windows(needle.len())
                .position(|v| v == needle)
                .unwrap()
        };
        assert_eq!(colors[at("git")], Some(COMMAND));
        assert_eq!(colors[at("--oneline")], Some(OPTION));
        assert_eq!(colors[at("中文")], Some(STRING));
        assert_eq!(colors[at("grep")], Some(COMMAND));
        assert_eq!(colors[at("$BRANCH")], Some(VARIABLE));
        assert_eq!(colors[at("# note")], Some(COMMENT));
    }
    #[test]
    fn incomplete_quotes_and_escaped_spaces_are_safe() {
        assert_eq!(
            colors(&"echo 'unfinished".chars().collect::<Vec<_>>()).last(),
            Some(&Some(ERROR))
        );
        assert_eq!(
            colors(&"FOO=bar git".chars().collect::<Vec<_>>()).last(),
            Some(&Some(COMMAND))
        );
        assert_eq!(
            colors(&"echo path\\ with\\ spaces".chars().collect::<Vec<_>>()).last(),
            Some(&None)
        );
    }
}
