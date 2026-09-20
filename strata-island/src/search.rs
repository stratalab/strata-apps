//! Replaceable application index (#3482/#3483). Persisted Strata records are authoritative.
use crate::{addresses::Address, error::IslandError, extract, places::Place};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
pub fn normalize(s: &str) -> String {
    let expanded = s
        .replace('½', " 1/2")
        .replace('¼', " 1/4")
        .replace('¾', " 3/4");
    let raw: String = expanded
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c == '⁄' {
                '/'
            } else if c.is_alphanumeric() || c == '-' || c == '/' {
                c
            } else if c == '–' || c == '—' {
                '-'
            } else {
                ' '
            }
        })
        .collect();
    raw.split_whitespace()
        .flat_map(|t| {
            if t.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                vec![t]
            } else {
                t.split('-').collect()
            }
        })
        .map(|t| match t {
            "st" => "street".into(),
            "ave" | "av" => "avenue".into(),
            "rd" => "road".into(),
            "blvd" => "boulevard".into(),
            "pl" => "place".into(),
            "dr" => "drive".into(),
            "ln" => "lane".into(),
            "sq" => "square".into(),
            "pkwy" => "parkway".into(),
            "ter" => "terrace".into(),
            "w" => "west".into(),
            "e" => "east".into(),
            "n" => "north".into(),
            "s" => "south".into(),
            "first" => "1".into(),
            "second" => "2".into(),
            "third" => "3".into(),
            "fourth" => "4".into(),
            "fifth" => "5".into(),
            "sixth" => "6".into(),
            "seventh" => "7".into(),
            "eighth" => "8".into(),
            "ninth" => "9".into(),
            "tenth" => "10".into(),
            "eleventh" => "11".into(),
            "twelfth" => "12".into(),
            _ => {
                let stripped = ["st", "nd", "rd", "th"].iter().find_map(|suffix| {
                    t.strip_suffix(suffix)
                        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                });
                stripped.unwrap_or(t).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
struct Document {
    place: Place,
    keys: Vec<String>,
    house: Option<String>,
    zip: Option<String>,
    linked: Option<String>,
}
pub struct Index {
    docs: Vec<Document>,
    postings: BTreeMap<String, Vec<usize>>,
    exact: BTreeMap<String, Vec<usize>>,
}
impl Index {
    pub fn new(addresses: &[Address], places: &[Place]) -> Self {
        let linked: BTreeMap<_, _> = addresses
            .iter()
            .flat_map(|a| a.place_ids.iter().map(move |p| (p.as_str(), a)))
            .collect();
        let mut docs = Vec::new();
        for a in addresses {
            docs.push(Document {
                place: a.place.clone(),
                keys: vec![normalize(&a.place.name)],
                house: Some(a.house.clone()),
                zip: a.zip.clone(),
                linked: a.place_ids.first().cloned(),
            });
        }
        for p in places {
            let a = linked.get(p.id.as_str());
            let mut keys = vec![normalize(&p.name)];
            keys.extend(p.aliases.iter().map(|s| normalize(s)));
            if let Some(a) = a {
                keys.push(normalize(&a.place.name));
            }
            docs.push(Document {
                place: p.clone(),
                keys,
                house: a.map(|a| a.house.clone()),
                zip: a.and_then(|a| a.zip.clone()),
                linked: a.map(|a| a.place.id.clone()),
            });
        }
        let mut postings: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut exact: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, d) in docs.iter().enumerate() {
            let tokens: BTreeSet<_> = d.keys.iter().flat_map(|k| k.split_whitespace()).collect();
            for token in tokens {
                postings.entry(token.into()).or_default().push(i);
                if let Ok(n) = token.parse::<usize>() {
                    if let Some(word) = [
                        "first", "second", "third", "fourth", "fifth", "sixth", "seventh",
                        "eighth", "ninth", "tenth", "eleventh", "twelfth",
                    ]
                    .get(n.wrapping_sub(1))
                    {
                        postings.entry((*word).into()).or_default().push(i);
                    }
                }
            }
            for k in &d.keys {
                exact.entry(k.clone()).or_default().push(i);
            }
        }
        Self {
            docs,
            postings,
            exact,
        }
    }
    pub fn query(
        &self,
        query: &str,
        category: Option<&str>,
        limit: usize,
        cursor: Option<&str>,
        branch: &str,
        version: u64,
    ) -> Result<Value, IslandError> {
        if query.len() > 200
            || !(1..=20).contains(&limit)
            || category.is_some_and(|c| !["address", "landmark", "park", "transit"].contains(&c))
        {
            return Err(IslandError::code("invalid_argument.island.search"));
        }
        let start = std::time::Instant::now();
        let mut q = normalize(query);
        let mut zip = None;
        if let Some(t) = q.split_whitespace().last() {
            if t.len() == 5 && t.bytes().all(|b| b.is_ascii_digit()) {
                zip = Some(t.to_owned());
                q = q[..q.len() - t.len()].trim().into();
            }
        }
        loop {
            let next = [
                " new york ny",
                " new york",
                " manhattan ny",
                " manhattan",
                " ny",
            ]
            .iter()
            .find_map(|s| q.strip_suffix(s).map(str::to_owned));
            if let Some(n) = next {
                q = n
            } else {
                break;
            }
        }
        let signature = format!(
            "{:016x}",
            extract::fnv1a64(
                format!("search-v2:{branch}:{version}:{q}:{zip:?}:{category:?}").as_bytes()
            )
        );
        let offset = match cursor {
            None => 0,
            Some(c) => c
                .split_once(':')
                .filter(|(s, _)| *s == signature)
                .and_then(|(_, n)| n.parse::<usize>().ok())
                .ok_or(IslandError::code("invalid_argument.island.cursor"))?,
        };
        let tokens: Vec<_> = q.split_whitespace().collect();
        let numeric = |t: &&str| t.as_bytes().first().is_some_and(u8::is_ascii_digit);
        let street_first = tokens
            .get(1)
            .is_some_and(|t| ["street", "avenue"].contains(t));
        let house = if street_first {
            tokens
                .last()
                .filter(|_| tokens.len() > 2)
                .filter(|t| numeric(t))
                .copied()
        } else if tokens.first().is_some_and(|t| numeric(t)) {
            tokens.first().copied()
        } else if tokens.len() > 1
            && !tokens
                .first()
                .is_some_and(|t| ["east", "west", "north", "south"].contains(t))
        {
            tokens.last().filter(|t| numeric(t)).copied()
        } else {
            None
        };
        let house = house.map(|h| {
            if tokens.first() == Some(&h) && tokens.get(1).is_some_and(|t| t.contains('/')) {
                format!("{} {}", h, tokens[1])
            } else {
                h.to_owned()
            }
        });
        let invalid = query.contains('#')
            || tokens
                .iter()
                .any(|t| ["apt", "apartment", "unit", "suite"].contains(t));
        let mut truncated = false;
        let mut candidates: BTreeSet<usize> = BTreeSet::new();
        if !invalid && q.len() >= 2 && (house.is_none() || tokens.len() > 1) {
            if let Some(exact) = self.exact.get(&q) {
                candidates.extend(exact);
            }
            // Begin with the most selective exact term, otherwise the smallest bounded prefix union.
            let mut lists = Vec::new();
            for token in &tokens {
                let exact_numeric = token.bytes().any(|b| b.is_ascii_digit());
                let mut list: BTreeSet<usize> = BTreeSet::new();
                if exact_numeric {
                    if let Some(ids) = self.postings.get(*token) {
                        list.extend(ids.iter().copied());
                    }
                } else {
                    for (terms, (key, ids)) in self
                        .postings
                        .range((*token).to_owned()..)
                        .take_while(|(k, _)| k.starts_with(token))
                        .enumerate()
                    {
                        if terms >= 128 {
                            truncated = true;
                            break;
                        }
                        let _ = key;
                        list.extend(ids.iter().copied());
                        // Posting sets can span the catalog; bounded catalog size, no per-document scan.
                    }
                }
                lists.push(list);
            }
            lists.sort_by_key(BTreeSet::len);
            if let Some(first) = lists.first() {
                for &i in first {
                    if lists.iter().skip(1).all(|s| s.contains(&i)) && {
                        let d = &self.docs[i];
                        category.is_none_or(|c| d.place.category == c)
                            && zip.as_ref().is_none_or(|z| d.zip.as_ref() == Some(z))
                            && house
                                .as_deref()
                                .is_none_or(|h| d.house.as_deref() == Some(h))
                    } {
                        if candidates.len() >= 5000 {
                            truncated = true;
                            break;
                        }
                        candidates.insert(i);
                    }
                }
            }
        }
        let mut results: Vec<_> = candidates
            .into_iter()
            .filter(|&i| {
                let d = &self.docs[i];
                category.is_none_or(|c| d.place.category == c)
                    && zip.as_ref().is_none_or(|z| d.zip.as_ref() == Some(z))
                    && house
                        .as_deref()
                        .is_none_or(|h| d.house.as_deref() == Some(h))
            })
            .collect();
        let matched_places: BTreeSet<_> = results
            .iter()
            .map(|&i| self.docs[i].place.id.as_str())
            .collect();
        results.retain(|&i| {
            let d = &self.docs[i];
            if house.is_some() {
                d.place.category == "address"
                    || d.linked
                        .as_deref()
                        .is_none_or(|p| !matched_places.contains(p))
            } else {
                d.place.category != "address"
                    || d.linked
                        .as_deref()
                        .is_none_or(|p| !matched_places.contains(p))
            }
        });
        results.sort_by_key(|&i| {
            let d = &self.docs[i];
            (
                !d.keys.iter().any(|k| k == &q),
                !d.keys.iter().any(|k| k.starts_with(&q)),
                house.is_none() && d.place.category == "address",
                d.place.category.clone(),
                d.place.name.clone(),
                d.place.id.clone(),
            )
        });
        let total = results.len();
        let next = offset.saturating_add(limit);
        let rows: Vec<_> = results
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|i| &self.docs[i].place)
            .collect();
        Ok(
            json!({"branch":branch,"version":version,"places":rows,"total":if truncated{None}else{Some(total)},"truncated":truncated,"cursor":if next<total{Some(format!("{signature}:{next}"))}else{None},"search_index":"application token/prefix index hydrated from Strata","search_ms":start.elapsed().as_secs_f64()*1000.,"hint":if invalid{Some("Search a street address without an apartment or unit.")}else if house.is_some()&&tokens.len()==1{Some("Add a street name.")}else{None}}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization() {
        assert_eq!(
            normalize("350 FIFTH Ave., New York"),
            "350 5 avenue new york"
        );
        assert_eq!(normalize("230 W 55th St"), "230 west 55 street");
        assert_eq!(normalize("Café 12–14"), "cafe 12-14");
    }
}
