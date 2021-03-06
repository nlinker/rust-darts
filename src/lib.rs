//  FileName    : lib.rs
//  Author      : ShuYu Wang <andelf@gmail.com>
//  Created     : Tue Sep 13 23:37:09 2016 by ShuYu Wang
//  Copyright   : Feather Workshop (c) 2016
//  Description : A double array trie implementation.
//  Time-stamp: <2016-09-13 23:38:14 andelf>

#![cfg_attr(feature = "dev", plugin(clippy))]
#![cfg_attr(not(feature = "dev"), allow(unknown_lints))]
#![feature(pattern)]
#![feature(test)]

extern crate test;
#[macro_use]
extern crate log;
extern crate rustc_serialize;
extern crate bincode;


use std::str;
use std::iter;
use std::vec;
use std::io;
use std::io::prelude::*;
use std::result;
use std::error;
use std::fmt;

use std::str::pattern::{Searcher, SearchStep};

use bincode::SizeLimit;
use bincode::rustc_serialize::{encode, decode};


/// The error type which is used in this crate.
#[derive(Debug)]
pub enum DartsError {
    Encoding(bincode::rustc_serialize::EncodingError),
    Decoding(bincode::rustc_serialize::DecodingError),
    Io(io::Error),
}

impl fmt::Display for DartsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rust-darts error")
    }
}

impl error::Error for DartsError {
    fn description(&self) -> &str {
        match *self {
            DartsError::Encoding(ref err) => err.description(),
            DartsError::Decoding(ref err) => err.description(),
            DartsError::Io(ref err) => err.description(),
        }
    }
    // fn cause(&self) -> Option<&Error> { ... }
}

/// The result type which is used in this crate.
pub type Result<T> = result::Result<T, DartsError>;

impl From<bincode::rustc_serialize::EncodingError> for DartsError {
    fn from(err: bincode::rustc_serialize::EncodingError) -> Self {
        DartsError::Encoding(err)
    }
}

impl From<bincode::rustc_serialize::DecodingError> for DartsError {
    fn from(err: bincode::rustc_serialize::DecodingError) -> Self {
        DartsError::Decoding(err)
    }
}

impl From<io::Error> for DartsError {
    fn from(err: io::Error) -> Self {
        DartsError::Io(err)
    }
}


#[inline]
fn max<T: PartialOrd + Copy>(a: T, b: T) -> T {
    if a > b {
        a
    } else {
        b
    }
}



struct Node {
    code: usize,
    depth: usize,
    left: usize,
    right: usize,
}

/// Build a Double Arrary Trie from a series of strings.
pub struct DoubleArrayTrieBuilder<'a> {
    check: Vec<u32>,
    base: Vec<i32>,
    used: Vec<bool>,

    size: usize,
    alloc_size: usize,
    keys: Vec<iter::Chain<str::Chars<'a>, vec::IntoIter<char>>>, // String::chars() iterator
    next_check_pos: usize,

    progress: usize,
    progress_func: Option<Box<Fn(usize, usize) -> ()>>,
}

impl<'a> DoubleArrayTrieBuilder<'a> {
    // FIXME: clippy complains
    pub fn new() -> DoubleArrayTrieBuilder<'a> {
        DoubleArrayTrieBuilder {
            check: vec![],
            base: vec![],
            used: vec![],
            size: 0,
            alloc_size: 0,
            keys: vec![],
            next_check_pos: 0,
            progress: 0,
            progress_func: None,
        }
    }

    /// Set callback to inspect trie building progress.
    pub fn progress<F>(mut self, func: F) -> DoubleArrayTrieBuilder<'a>
        where F: 'static + Fn(usize, usize) -> ()
    {

        self.progress_func = Some(Box::new(func));
        self
    }

    // pub fn build(mut self, keys: &[&str], values: &[usize]) -> DoubleArrayTrie {
    pub fn build(mut self, keys: &'a [&str]) -> DoubleArrayTrie {
        // must be size of single store unit
        self.resize(std::char::MAX as usize);

        self.keys = keys.iter()
                        .map(|s| s.chars().chain(vec!['\u{0}']))
                        .collect();

        self.base[0] = 1;
        self.next_check_pos = 0;

        let root_node = Node {
            code: 0,
            left: 0,
            right: keys.len(),
            depth: 0,
        };

        let mut siblings = Vec::new();
        self.fetch(&root_node, &mut siblings);
        self.insert(&siblings);

        // shrink size, free the unnecessary memory
        let last_used_pos = self.used
                                .iter()
                                .enumerate()
                                .rev()
                                .find(|&(_, &k)| k)
                                .map_or(self.alloc_size, |t| t.0 + std::char::MAX as usize);
        self.resize(last_used_pos);

        let DoubleArrayTrieBuilder { check, base, .. } = self;
        DoubleArrayTrie {
            check: check,
            base: base,
        }
    }

    fn resize(&mut self, new_len: usize) {
        self.check.resize(new_len, 0);
        self.base.resize(new_len, 0);
        self.used.resize(new_len, false);

        self.alloc_size = new_len;
    }

    fn fetch(&mut self, parent: &Node, siblings: &mut Vec<Node>) -> usize {
        let mut prev = 0;

        for i in parent.left..parent.right {
            let c = self.keys[i].next();

            if c.is_none() {
                continue;
            }

            let curr = c.map_or(0, |c| {
                if c != '\u{0}' {
                    c as usize + 1   // +1 for that 0 used as NULL
                } else {
                    0 // 0表示结束状态
                }
            });

            assert!(prev <= curr, "keys must be sorted!");

            if curr != prev || siblings.is_empty() {
                let tmp_node = Node {
                    code: curr,
                    depth: parent.depth + 1,
                    left: i,
                    right: 0,
                };

                siblings.last_mut().map(|n| n.right = i);
                siblings.push(tmp_node);
            }

            prev = curr;
        }

        siblings.last_mut().map(|n| n.right = parent.right);
        siblings.len()
    }

    fn insert(&mut self, siblings: &[Node]) -> usize {

        let mut begin: usize;
        let mut pos = max(siblings[0].code + 1, self.next_check_pos) - 1;
        let mut nonzero_num = 0;
        let mut first = 0;
        let key_size = self.keys.len();

        if self.alloc_size <= pos {
            self.resize(pos + 1);
        }

        'outer: loop {
            pos += 1;

            if self.alloc_size <= pos {
                self.resize(pos + 1);
            }

            if self.check[pos] != 0 {
                nonzero_num += 1;
                continue;
            } else if first == 0 {
                self.next_check_pos = pos;
                first = 1;
            }

            begin = pos - siblings[0].code;

            if self.alloc_size <= begin + siblings.last().map(|n| n.code).unwrap() {
                let l = (self.alloc_size as f32) *
                        max(1.05, key_size as f32 / (self.progress as f32 + 1.0));
                self.resize(l as usize)
            }

            if self.used[begin] {
                continue;
            }

            for n in siblings.iter() {
                if self.check[begin + n.code] != 0 {
                    continue 'outer;
                }
            }

            break;
        }

        // Simple heuristics
        // 从位置 next_check_pos 开始到 pos 间，如果已占用的空间在95%以上，
        // 下次插入节点时，直接从 pos 位置处开始查找
        if nonzero_num as f32 / (pos as f32 - self.next_check_pos as f32 + 1.0) >= 0.95 {
            self.next_check_pos = pos;
        }

        self.used[begin] = true;
        self.size = max(self.size,
                        begin + siblings.last().map(|n| n.code).unwrap() + 1);

        siblings.iter().map(|n| self.check[begin + n.code] = begin as u32).last();

        for sibling in siblings.iter() {
            let mut new_siblings = Vec::new();

            // 一个词的终止且不为其他词的前缀，其实就是叶子节点
            if self.fetch(sibling, &mut new_siblings) == 0 {
                // FIXME: ignore value ***
                self.base[begin + sibling.code] = -(sibling.left as i32) - 1;
                self.progress += 1;
                self.progress_func.as_ref().map(|f| f(self.progress, key_size));

            } else {
                let h = self.insert(&new_siblings);
                self.base[begin + sibling.code] = h as i32;
            }
        }

        begin
    }
}

/// A Double Array Trie.
#[derive(Debug, RustcEncodable, RustcDecodable)]
pub struct DoubleArrayTrie {
    base: Vec<i32>, // use negetive to indicate ends
    check: Vec<u32>,
}


impl DoubleArrayTrie {
    /// Match whole string.
    pub fn exact_match_search(&self, key: &str) -> Option<usize> {
        let mut b = self.base[0];
        let mut p: usize;

        for c in key.chars() {
            p = (b + c as i32 + 1) as usize;

            if b == self.check[p] as i32 {
                b = self.base[p];
            } else {
                return None;
            }
        }

        p = b as usize;
        let n = self.base[p];

        if b == self.check[p] as i32 && n < 0 {
            Some((-n - 1) as usize)
        } else {
            None
        }
    }

    /// Find all matched prefixes. Returns [(end_index, value)].
    pub fn common_prefix_search(&self, key: &str) -> Option<Vec<(usize, usize)>> {
        let mut result = vec![];

        let mut b = self.base[0];
        let mut n;
        let mut p: usize;

        for (i, c) in key.char_indices() {
            p = b as usize;
            n = self.base[p];

            if b == self.check[p] as i32 && n < 0 {
                result.push((i, (-n - 1) as usize))
            }

            p = b as usize + c as usize + 1;
            if b == self.check[p] as i32 {
                b = self.base[p];
            } else {
                return if result.is_empty() {
                    None
                } else {
                    Some(result)
                };
            }
        }

        p = b as usize;
        n = self.base[p];

        if b == self.check[p] as i32 && n < 0 {
            result.push((key.len(), (-n - 1) as usize));
            Some(result)
        } else {
            None
        }
    }

    /// Save DAT to an output stream.
    pub fn save<W: Write>(&self, w: &mut W) -> Result<()> {
        let encoded: Vec<u8> = try!(encode(self, SizeLimit::Infinite));
        Ok(try!(w.write_all(&encoded)))
    }

    /// Load DAT from input stream.
    pub fn load<R: Read>(r: &mut R) -> Result<Self> {
        let mut buf = Vec::new();
        let _ = try!(r.read_to_end(&mut buf));
        Ok(try!(decode(&buf)))
    }

    /// Run Forward Maximum Matching Method on a string. Returns a Searcher.
    pub fn search<'a, 'b>(&'b self, haystack: &'a str) -> DoubleArrayTrieSearcher<'a, 'b> {
        DoubleArrayTrieSearcher {
            haystack: haystack,
            dat: self,
            start_pos: 0,
        }
    }
}


/// A seracher for all words in Double Array Trie, using Forward Maximum Matching Method.
pub struct DoubleArrayTrieSearcher<'a, 'b> {
    haystack: &'a str,
    dat: &'b DoubleArrayTrie,
    start_pos: usize,
}

impl<'a, 'b> DoubleArrayTrieSearcher<'a, 'b> {
    pub fn search_step_to_str(&self, step: &SearchStep) -> String {
        match *step {
            SearchStep::Match(start, end) => format!("{}/n", &self.haystack()[start..end]),
            SearchStep::Reject(start, end) => format!("{}/x", &self.haystack()[start..end]),
            _ => "/#".into(),
        }
    }
}


unsafe impl<'a, 'b> Searcher<'a> for DoubleArrayTrieSearcher<'a, 'b> {
    fn haystack(&self) -> &'a str {
        self.haystack
    }

    fn next(&mut self) -> SearchStep {
        let base = &self.dat.base;
        let check = &self.dat.check;

        let mut b = base[0];
        let mut n;
        let mut p: usize;

        let start_pos = self.start_pos;

        let mut next_pos = 0;
        let mut result = None;

        if start_pos >= self.haystack.len() {
            return SearchStep::Done;
        }

        for (i, c) in self.haystack[start_pos..].char_indices() {
            p = b as usize;
            n = base[p];

            if b == check[p] as i32 && n < 0 {
                next_pos = start_pos + i;
                result = Some(SearchStep::Match(start_pos, start_pos + i));
            }

            p = b as usize + c as usize + 1;
            if b == check[p] as i32 {
                b = base[p];
            } else if result.is_some() {
                // last item is the maximum matching
                self.start_pos = next_pos;
                return result.unwrap();
            } else {
                self.start_pos = start_pos + i + c.len_utf8();
                return SearchStep::Reject(start_pos, self.start_pos);
            }
        }

        p = b as usize;
        n = base[p];

        // full match from start to end
        self.start_pos = self.haystack.len();
        if b == check[p] as i32 && n < 0 {
            SearchStep::Match(start_pos, self.start_pos)
        } else {
            SearchStep::Reject(start_pos, self.start_pos)
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::prelude::*;
    use std::io::BufReader;
    use std::fs::File;
    use std::str::pattern::{Searcher, SearchStep};

    use test::Bencher;


    #[test]
    fn test_dat_basic() {
        let f = File::open("./priv/dict.txt.big").unwrap();

        let mut keys: Vec<String> = BufReader::new(f)
                                        .lines()
                                        .map(|s| s.unwrap())
                                        .collect();
        keys.sort();

        let strs: Vec<&str> = keys.iter()
                                  .map(|n| n.split(' ').next().unwrap())
                                  .collect();

        let da = DoubleArrayTrieBuilder::new()
                     .progress(|c, t| println!("{}/{}", c, t))
                     .build(&strs);

        let _ = File::create("./priv/dict.big.bincode")
                    .as_mut()
                    .map(|f| da.save(f))
                    .expect("write ok!");

        assert!(da.exact_match_search("she").is_none());
        assert!(da.exact_match_search("万能胶啥").is_none());
        assert!(da.exact_match_search("呼伦贝尔").is_some());
        assert!(da.exact_match_search("东湖高新技术开发区").is_some());

    }

    #[bench]
    fn bench_dat_prefix_search(b: &mut Bencher) {
        let mut f = File::open("./priv/dict.big.bincode").unwrap();
        let da = DoubleArrayTrie::load(&mut f).unwrap();

        b.iter(|| da.common_prefix_search("中华人民共和国").unwrap());
    }

    #[test]
    fn test_dat_prefix_search() {
        let mut f = File::open("./priv/dict.big.bincode").unwrap();
        let da = DoubleArrayTrie::load(&mut f).unwrap();

        let string = "中华人民共和国";
        da.common_prefix_search(string)
          .as_ref()
          .map(|matches| {
              matches.iter()
                     .map(|&(end_idx, v)| {
                         println!("prefix[{}] = {}", &string[..end_idx], v);
                     })
                     .last()
          });
    }

    #[test]
    fn test_dat_searcher() {
        let mut f = File::open("./priv/dict.big.bincode").unwrap();
        let da = DoubleArrayTrie::load(&mut f).unwrap();

        let text = "江西鄱阳湖干枯，中国最大淡水湖变成大草原";
        let mut searcher = da.search(&text);

        let mut result = vec![];
        loop {
            let step = searcher.next();
            // println!("{:?}\t{}", step, searcher.search_step_to_str(&step));
            if step == SearchStep::Done {
                break;
            }
            result.push(step);
        }
        let segmented = result.iter()
                              .map(|s| searcher.search_step_to_str(s))
                              .collect::<Vec<String>>()
                              .join(" ");
        assert_eq!(segmented,
                   "江西/n 鄱阳湖/n 干枯/n ，/x 中国/n 最大/n 淡水湖/n 变成/n 大/n 草原/n");
    }

    #[bench]
    fn bench_dat_searcher(b: &mut Bencher) {
        let mut f = File::open("./priv/dict.big.bincode").unwrap();
        let da = DoubleArrayTrie::load(&mut f).unwrap();

        let mut f = File::open("./priv/《我的团长我的团》全集.txt").unwrap();
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();

        let mut searcher = da.search(&text);
        assert!(text.len() > 0);

        b.iter(|| loop {
            let step = searcher.next();
            if step == SearchStep::Done {
                break;
            }
        });
        // MacBook Pro (Retina, 15-inch, Mid 2014)
        // bench:   7,572,550 ns/iter (+/- 1,715,688)
    }

    #[bench]
    fn bench_dat_build(b: &mut Bencher) {
        let f = File::open("./priv/dict.txt.big").unwrap();
        let keys: Vec<String> = BufReader::new(f)
                                    .lines()
                                    .map(|s| s.unwrap())
                                    .collect();

        let strs: Vec<&str> = keys.iter()
                                  .map(|n| n.split(' ').next().unwrap())
                                  .collect();

        b.iter(|| DoubleArrayTrieBuilder::new().build(&strs));
    }


    #[bench]
    fn bench_dat_match_found(b: &mut Bencher) {
        let mut f = File::open("./priv/dict.big.bincode").unwrap();
        let da = DoubleArrayTrie::load(&mut f).unwrap();

        b.iter(|| da.exact_match_search("东湖高新技术开发区").unwrap());
    }

    #[bench]
    fn bench_dat_match_not_found(b: &mut Bencher) {
        let mut f = File::open("./priv/dict.big.bincode").unwrap();
        let da = DoubleArrayTrie::load(&mut f).unwrap();

        b.iter(|| da.exact_match_search("东湖高新技术开发区#"));
    }
}
