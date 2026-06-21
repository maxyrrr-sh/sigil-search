//! Pipe-DSL → SQL lowering (DESIGN §9).
//!
//! A deliberately small SPL/KQL-style subset that lowers to the same SQL the
//! DataFusion engine runs, so DSL and SQL share one execution path:
//!
//! ```text
//! search <text> | where <field> <op> <value> | stats count [by <field>]
//!               | fields a,b | sort <field> [desc] | head <n>
//! ```
//!
//! Operators: `==` `!=` `=` `contains`. Field aliases: `log.level`→`log_level`,
//! `host.name`/`host`→`host`.

/// Lower a pipe-DSL string into a SQL query over the `events` table.
pub fn lower(pipeline: &str) -> anyhow::Result<String> {
    let mut select = "*".to_string();
    let mut wheres: Vec<String> = Vec::new();
    let mut group: Option<String> = None;
    let mut order: Option<String> = None;
    let mut limit: Option<usize> = None;

    for stage in pipeline.split('|') {
        let stage = stage.trim();
        if stage.is_empty() {
            continue;
        }
        let (cmd, rest) = split_first_word(stage);
        match cmd {
            "search" => {
                if !rest.is_empty() {
                    wheres.push(format!("message LIKE '%{}%'", escape(rest)));
                }
            }
            "where" => wheres.push(translate_predicate(rest)?),
            "stats" => {
                let (sel, grp) = parse_stats(rest)?;
                select = sel;
                group = grp;
            }
            "fields" => {
                let cols: Vec<String> = rest
                    .split(',')
                    .map(|c| map_field(c.trim()))
                    .filter(|c| !c.is_empty())
                    .collect();
                if !cols.is_empty() {
                    select = cols.join(", ");
                }
            }
            "sort" => order = Some(translate_sort(rest)),
            "head" | "limit" => {
                limit = rest
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("`{cmd}` needs a number, got '{rest}'"))
                    .map(Some)?;
            }
            other => anyhow::bail!("unknown DSL command '{other}'"),
        }
    }

    let mut sql = format!("SELECT {select} FROM events");
    if !wheres.is_empty() {
        sql.push_str(&format!(" WHERE {}", wheres.join(" AND ")));
    }
    if let Some(g) = group {
        sql.push_str(&format!(" GROUP BY {g}"));
    }
    if let Some(o) = order {
        sql.push_str(&format!(" ORDER BY {o}"));
    }
    if let Some(l) = limit {
        sql.push_str(&format!(" LIMIT {l}"));
    }
    Ok(sql)
}

fn split_first_word(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, ""),
    }
}

fn translate_predicate(expr: &str) -> anyhow::Result<String> {
    for (op, sql_op) in [("==", "="), ("!=", "!="), (" contains ", "LIKE"), ("=", "=")] {
        if let Some(i) = expr.find(op) {
            let field = map_field(expr[..i].trim());
            let raw = expr[i + op.len()..].trim();
            let value = unquote(raw);
            if sql_op == "LIKE" {
                return Ok(format!("{field} LIKE '%{}%'", escape(&value)));
            }
            return Ok(format!("{field} {sql_op} {}", literal(&value)));
        }
    }
    anyhow::bail!("unparsable `where` expression: {expr}")
}

/// `count` or `count by <field>` → (select-list, group-by).
fn parse_stats(rest: &str) -> anyhow::Result<(String, Option<String>)> {
    let rest = rest.trim();
    let (agg, by) = match rest.split_once(" by ") {
        Some((a, b)) => (a.trim(), Some(map_field(b.trim()))),
        None => (rest, None),
    };
    if agg != "count" {
        anyhow::bail!("only `stats count [by <field>]` is supported, got '{agg}'");
    }
    let select = match &by {
        Some(field) => format!("{field}, count(*) as count"),
        None => "count(*) as count".to_string(),
    };
    Ok((select, by))
}

fn translate_sort(rest: &str) -> String {
    let rest = rest.trim();
    if let Some(field) = rest.strip_suffix(" desc") {
        format!("{} DESC", map_field(field.trim()))
    } else if let Some(field) = rest.strip_suffix(" asc") {
        format!("{} ASC", map_field(field.trim()))
    } else {
        map_field(rest)
    }
}

fn map_field(field: &str) -> String {
    match field {
        "log.level" => "log_level".to_string(),
        "host.name" | "host" => "host".to_string(),
        other => other.to_string(),
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// A SQL literal: bare for numbers, single-quoted (escaped) otherwise.
fn literal(value: &str) -> String {
    if value.parse::<f64>().is_ok() {
        value.to_string()
    } else {
        format!("'{}'", escape(value))
    }
}

fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_search_where_head() {
        let sql = lower("search login | where log.level == error | head 5").unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM events WHERE message LIKE '%login%' AND log_level = 'error' LIMIT 5"
        );
    }

    #[test]
    fn lowers_stats_by() {
        let sql = lower("stats count by dataset | sort count desc").unwrap();
        assert_eq!(
            sql,
            "SELECT dataset, count(*) as count FROM events GROUP BY dataset ORDER BY count DESC"
        );
    }

    #[test]
    fn numeric_literal_unquoted() {
        let sql = lower("where ts != 0").unwrap();
        assert_eq!(sql, "SELECT * FROM events WHERE ts != 0");
    }
}
