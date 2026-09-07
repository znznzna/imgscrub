//! セグメント単位の保持 / 削除の判定。
//!
//! 判定はセグメント階層の allowlist（保持リストに載ったものだけを通す）で行う。
//! 保持したセグメントの内部に対するプロパティ単位の除去は `crate::xmp` が担う。

use super::segment::{App, Segment};

/// 除去の理由。報告に使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    /// C2PA マニフェスト（APP11/JUMBF）
    C2pa,
    /// MPF は先行セグメントの削除で絶対オフセットが破綻するため削除する
    MpfOffsetsInvalidated,
    /// 未知のベンダー拡張。allowlist 思想により削除する
    UnknownApp(u8),
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::C2pa => write!(f, "C2PA マニフェスト"),
            Self::MpfOffsetsInvalidated => {
                write!(f, "MPF（先行セグメント削除でオフセットが無効になる）")
            }
            Self::UnknownApp(n) => write!(f, "未知の APP{n}"),
        }
    }
}

/// 1 セグメントに対する処理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// バイト列をそのまま通す
    Keep,
    /// 削除する
    Drop(DropReason),
    /// 内部をプロパティ単位で書き換える（XMP のみ）
    Rewrite,
}

/// 除去の強さ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// APP11 のみを対象にし、XMP も未知の APPn も触らない
    pub c2pa_only: bool,
}

/// セグメント列に対する処理を決める。
///
/// MPF は「先行セグメントが 1 つでも削除されたら削除する」。オフセットが絶対値で
/// 格納されているため、前を詰めると必ず壊れる（設計書 §4.3）。逆に何も削除しないなら
/// そのまま残せる。
pub fn decide(data: &[u8], segs: &[Segment], opts: &Options) -> Vec<Action> {
    let mut actions = Vec::with_capacity(segs.len());
    let mut dropped_before = false;

    for s in segs {
        let action = match s.app_kind(data) {
            // APPn 以外（DQT / SOF / DHT / SOS など）は常に無編集で通す
            None => Action::Keep,

            Some(App::Jumbf) => Action::Drop(DropReason::C2pa),

            Some(App::Mpf) => {
                if dropped_before {
                    Action::Drop(DropReason::MpfOffsetsInvalidated)
                } else {
                    Action::Keep
                }
            }

            Some(App::Xmp) => {
                if opts.c2pa_only {
                    Action::Keep
                } else {
                    Action::Rewrite
                }
            }

            // 色と著作権に関わるものは保持する。APP14 を落とすと色解釈が変わる
            Some(App::Exif | App::Icc | App::Photoshop | App::Adobe | App::Jfif) => Action::Keep,

            // 拡張 XMP は安全に編集できないため保持する（呼び出し側が警告する）
            Some(App::XmpExtension) => Action::Keep,

            Some(App::Unknown(n)) => {
                if opts.c2pa_only {
                    Action::Keep
                } else {
                    Action::Drop(DropReason::UnknownApp(n))
                }
            }
        };

        if matches!(action, Action::Drop(_)) {
            dropped_before = true;
        }
        actions.push(action);
    }

    actions
}
