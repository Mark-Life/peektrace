//! Readable project labels: slugs minus the prefix they share (the home
//! dir, in slug form).

const SEP: char = '-';

/// Shared leading `-` segments across slugs, always leaving at least the
/// last segment of the shortest slug, so `app` / `api` never shrink to `p` /
/// `i`. A lone slug has no prefix: a `-` in its folder name looks the same
/// as a path separator, so any cut could split the name.
fn common_prefix(slugs: &[String]) -> String {
    let parts: Vec<Vec<&str>> = slugs.iter().map(|s| s.split(SEP).collect()).collect();
    let Some(first) = parts.first().filter(|_| parts.len() >= 2) else {
        return String::new();
    };
    let limit = parts
        .iter()
        .map(Vec::len)
        .min()
        .unwrap_or(0)
        .saturating_sub(1);
    let n = (0..limit)
        .take_while(|&i| parts.iter().all(|p| p[i] == first[i]))
        .count();
    if n == 0 {
        String::new()
    } else {
        format!("{}{SEP}", first[..n].join("-"))
    }
}

fn pretty_label(slug: &str, prefix: &str) -> String {
    let stripped = if prefix.is_empty() {
        slug.strip_prefix(SEP).unwrap_or(slug)
    } else {
        slug.get(prefix.len()..).unwrap_or("")
    };
    if stripped.is_empty() { slug } else { stripped }.to_owned()
}

/// `(slug, label)` for every slug, with the shared prefix stripped.
pub fn label_slugs(slugs: &[String]) -> Vec<(String, String)> {
    let prefix = common_prefix(slugs);
    slugs
        .iter()
        .map(|s| (s.clone(), pretty_label(s, &prefix)))
        .collect()
}

/// The label of `slug` in `labels`, or the slug itself.
pub fn label_of(labels: &[(String, String)], slug: &str) -> String {
    labels
        .iter()
        .find(|(s, _)| s == slug)
        .map_or_else(|| slug.to_owned(), |(_, l)| l.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(slugs: &[&str]) -> Vec<String> {
        let owned: Vec<String> = slugs.iter().map(|s| (*s).to_owned()).collect();
        label_slugs(&owned).into_iter().map(|(_, l)| l).collect()
    }

    #[test]
    fn no_slugs_gives_no_labels() {
        assert_eq!(labels(&[]), Vec::<String>::new());
    }

    #[test]
    fn a_single_project_keeps_its_whole_slug() {
        assert_eq!(
            labels(&["-home-dev-code-my-proj"]),
            ["home-dev-code-my-proj"]
        );
    }

    #[test]
    fn strips_the_prefix_at_segment_boundaries() {
        assert_eq!(
            labels(&["-home-dev-code-app", "-home-dev-code-api"]),
            ["app", "api"]
        );
        assert_eq!(
            labels(&["-home-dev-code-peektrace", "-home-dev-code-peektrace-s3"]),
            ["peektrace", "peektrace-s3"]
        );
    }

    #[test]
    fn a_slug_equal_to_the_shared_prefix_keeps_its_last_segment() {
        assert_eq!(
            labels(&["-home-dev-code", "-home-dev-code-web"]),
            ["code", "code-web"]
        );
    }

    #[test]
    fn slugs_with_nothing_in_common_only_lose_the_leading_dash() {
        assert_eq!(labels(&["-home-dev-a", "-srv-b"]), ["home-dev-a", "srv-b"]);
        assert_eq!(labels(&["-a-b", "C#proj"]), ["a-b", "C#proj"]);
    }
}
