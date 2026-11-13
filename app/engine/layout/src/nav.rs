//! 글자·어절 이동과 문서 순서. 위치는 문서 글 기준이다.

use std::cmp::Ordering;

use crate::query::stops;
use crate::{Layouter, Pos};

impl Layouter {
    /// 문단의 글자 묶음 경계(문서 글 기준, 오름차순).
    fn bounds(&self, pi: usize) -> Vec<usize> {
        let pb = self.pb(pi);
        let mut out: Vec<usize> = (0..pb.lines.len())
            .flat_map(|li| stops(pb, li))
            .map(|(q, _)| pb.to_doc(q))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// 문서 위치로 바꾼 (문단 번호, 위치).
    fn at(&self, pos: &Pos) -> Option<(usize, usize)> {
        let (pi, q) = self.resolve(pos)?;
        Some((pi, self.pb(pi).to_doc(q)))
    }

    fn para_start(&self, pi: usize) -> Option<Pos> {
        Some(Pos::new(self.paras.get(pi)?, 0))
    }

    fn para_end(&self, pi: usize) -> Pos {
        Pos::new(&self.paras[pi], self.pb(pi).src.len())
    }

    /// 다음 글자(자소 묶음 단위). 문단 끝에서는 다음 문단의 처음.
    pub fn next(&self, pos: &Pos) -> Option<Pos> {
        let (pi, off) = self.at(pos)?;
        match self.bounds(pi).into_iter().find(|&b| b > off) {
            Some(b) => Some(Pos::new(&pos.para, b)),
            None => self.para_start(pi + 1),
        }
    }

    /// 앞 글자. 문단 처음에서는 앞 문단의 끝.
    pub fn prev(&self, pos: &Pos) -> Option<Pos> {
        let (pi, off) = self.at(pos)?;
        match self.bounds(pi).into_iter().rev().find(|&b| b < off) {
            Some(b) => Some(Pos::new(&pos.para, b)),
            None => Some(self.para_end(pi.checked_sub(1)?)),
        }
    }

    /// 다음 어절의 끝(공백을 건너뛰고 공백 아닌 글자를 건너뛴 자리). 문단 끝에서는 다음 문단의 처음.
    pub fn next_word(&self, pos: &Pos) -> Option<Pos> {
        let (pi, off) = self.at(pos)?;
        let src = &self.pb(pi).src;
        if off >= src.len() {
            return self.para_start(pi + 1);
        }
        let rest = &src[off..];
        let ws = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        let word = rest[ws..]
            .find(char::is_whitespace)
            .unwrap_or(rest.len() - ws);
        let end = off + ws + word;
        let b = self.bounds(pi).into_iter().find(|&b| b >= end);
        Some(Pos::new(&pos.para, b.unwrap_or(src.len())))
    }

    /// 앞 어절의 처음. 문단 처음에서는 앞 문단의 끝.
    pub fn prev_word(&self, pos: &Pos) -> Option<Pos> {
        let (pi, off) = self.at(pos)?;
        if off == 0 {
            return Some(self.para_end(pi.checked_sub(1)?));
        }
        let head = &self.pb(pi).src[..off];
        let ws = head.trim_end().len();
        let start = head[..ws].rfind(char::is_whitespace).map_or(0, |i| {
            i + head[i..].chars().next().map_or(1, char::len_utf8)
        });
        let b = self.bounds(pi).into_iter().rev().find(|&b| b <= start);
        Some(Pos::new(&pos.para, b.unwrap_or(0)))
    }

    /// 두 번 누른 자리의 어절(공백 아닌 글자의 연속). 공백 위면 공백의 연속.
    pub fn word_at(&self, pos: &Pos) -> (Pos, Pos) {
        let Some((pi, off)) = self.at(pos) else {
            return (pos.clone(), pos.clone());
        };
        let src = &self.pb(pi).src;
        let after = src[off..].chars().next();
        let before = src[..off].chars().next_back();
        let space = match (before, after) {
            (_, Some(c)) if !c.is_whitespace() => false,
            (Some(c), _) if !c.is_whitespace() => false,
            (None, None) => return (Pos::new(&pos.para, off), Pos::new(&pos.para, off)),
            _ => true,
        };
        let same = |c: char| c.is_whitespace() == space;
        let start = src[..off]
            .char_indices()
            .rev()
            .take_while(|(_, c)| same(*c))
            .last()
            .map_or(off, |(i, _)| i);
        let end = off
            + src[off..]
                .find(|c: char| !same(c))
                .unwrap_or(src.len() - off);
        let bounds = self.bounds(pi);
        let s = bounds.iter().rev().find(|&&b| b <= start).copied();
        let e = bounds.iter().find(|&&b| b >= end).copied();
        (
            Pos::new(&pos.para, s.unwrap_or(0)),
            Pos::new(&pos.para, e.unwrap_or(src.len())),
        )
    }

    /// 문서 순서. 배치에 없는 문단은 뒤로 보낸다(id 순).
    pub fn order(&self, a: &Pos, b: &Pos) -> Ordering {
        let rank = |p: &Pos| self.index.get(&p.para).copied().unwrap_or(usize::MAX);
        (rank(a), a.para.as_str(), a.off).cmp(&(rank(b), b.para.as_str(), b.off))
    }

    /// 배치한 문단 id를 문서 순서로(본문의 문단만. 표 안의 문단은 없다).
    pub fn paras(&self) -> &[String] {
        &self.paras
    }

    /// 문단의 `paras()` 안 자리. 배치에 없는 문단이면 `None`.
    pub fn para_index(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }
}
