/// Computes the Levenshtein edit distance between two string slices.
///
/// # Algorithm
/// The Levenshtein distance between two strings `a` and `b` is the minimum number of
/// single-character edit operations (insertions, deletions, or substitutions) required
/// to transform `a` into `b`.
///
/// Let $m = |a|$ and $n = |b|$ in Unicode codepoints (`char`s).
/// Using dynamic programming:
/// - $d[i][0] = i$ (deleting all characters of prefix $a[0..i]$)
/// - $d[0][j] = j$ (inserting all characters of prefix $b[0..j]$)
/// - If $a[i-1] == b[j-1]$:
///     $d[i][j] = d[i-1][j-1]$
/// - If $a[i-1] \neq b[j-1]$:
///     $d[i][j] = 1 + \min(d[i-1][j], d[i][j-1], d[i-1][j-1])$
///
/// # Complexity
/// - **Time Complexity**: $O(m \times n)$ where $m$ and $n$ are the number of characters in `a` and `b`.
///   Each cell in the DP table is computed with constant-time operations.
/// - **Space Complexity**: $O(\min(m, n))$ memory.
///   Since computing row $i$ only depends on row $i - 1$, the implementation maintains
///   a single vector of size $\min(m, n) + 1$, optimizing memory usage.
///
/// # Unicode Handling
/// The algorithm decomposes strings into Unicode scalar values (`chars()`), ensuring
/// that multi-byte characters, accented letters, and non-ASCII scripts are measured
/// by character count rather than raw byte count.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    levenshtein_chars(&a_chars, &b_chars)
}

/// Internal helper operating on character slices.
pub fn levenshtein_chars(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    // Ensure `b` is the shorter slice to optimize space to O(min(m, n))
    let (a, b) = if a.len() < b.len() { (b, a) } else { (a, b) };

    let n = b.len();
    let mut dp: Vec<usize> = (0..=n).collect();

    for (i, &ca) in a.iter().enumerate() {
        let mut prev = dp[0];
        dp[0] = i + 1;

        for (j, &cb) in b.iter().enumerate() {
            let temp = dp[j + 1];
            if ca == cb {
                dp[j + 1] = prev;
            } else {
                // min of deletion (dp[j+1]), insertion (dp[j]), substitution (prev)
                dp[j + 1] = 1 + prev.min(dp[j]).min(dp[j + 1]);
            }
            prev = temp;
        }
    }

    dp[n]
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_empty_strings() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("a", ""), 1);
        assert_eq!(levenshtein("", "a"), 1);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn test_identical_strings() {
        assert_eq!(levenshtein("src", "src"), 0);
        assert_eq!(levenshtein("documents", "documents"), 0);
        assert_eq!(levenshtein("cmdmind", "cmdmind"), 0);
    }

    #[test]
    fn test_single_edits() {
        // Insertion
        assert_eq!(levenshtein("sc", "src"), 1);
        assert_eq!(levenshtein("src", "srcc"), 1);

        // Deletion
        assert_eq!(levenshtein("srcc", "src"), 1);
        assert_eq!(levenshtein("documnts", "documents"), 1);

        // Substitution
        assert_eq!(levenshtein("sbc", "src"), 1);
    }

    #[test]
    fn test_multiple_edits() {
        assert_eq!(levenshtein("pyhton", "python"), 2);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("flaw", "lawn"), 2);
        assert_eq!(levenshtein("gumbo", "gambol"), 2);
    }

    #[test]
    fn test_unicode_characters() {
        assert_eq!(levenshtein("café", "cafe"), 1);
        assert_eq!(levenshtein("résumé", "resume"), 2);
        assert_eq!(levenshtein("🦀", "🚀"), 1);
        assert_eq!(levenshtein("🦀rust", "🦀rust"), 0);
        assert_eq!(levenshtein("🦀rust", "rust"), 1);
    }

    #[test]
    fn test_case_sensitivity() {
        // Levenshtein on exact characters treats case changes as substitutions
        assert_eq!(levenshtein("src", "Src"), 1);
        assert_eq!(levenshtein("src", "SRC"), 3);
    }
}
