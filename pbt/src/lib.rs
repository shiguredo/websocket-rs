//! pbt クレートの共有サンプラー。
//!
//! `tests/prop_*.rs` のプロパティから使う。プロパティ本体と表明は各テストに書き、
//! ここには長さ・文字クラス・バイト列のように繰り返し使う引き方だけを置く。
//!
//! noprop は失敗したケースの再現情報を panic = "unwind" で残す。
//! テストプロファイルの既定が unwind なので、ここには設定を置かない。

/// 1 プロパティあたりの受理ケース数。
pub const CASES: usize = 256;

/// 失敗を再現するときに上書きする環境変数。
pub const SEED_ENV: &str = "SHIGUREDO_WEBSOCKET_SEED";

/// 境界値を選ぶ割合。5 回に 1 回。
const BOUNDARY_RATIO: noprop::Ratio = noprop::Ratio::one_nth(5);

/// `SHIGUREDO_WEBSOCKET_SEED` または時刻から seed を取り、[`CASES`] 件実行する。
///
/// 戻り値の [`noprop::Runner`] は、実行後のカバレッジゲートを表明するときに使う。
pub fn run(
    property: impl Fn(&mut noprop::TestCaseContext) -> noprop::TestResult,
) -> noprop::TestResult<noprop::Runner> {
    run_cases(CASES, property)
}

/// [`run`] と同じだが、ケース数を指定する。入力が定数だけのプロパティは 1 で足りる。
pub fn run_cases(
    cases: usize,
    property: impl Fn(&mut noprop::TestCaseContext) -> noprop::TestResult,
) -> noprop::TestResult<noprop::Runner> {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(cases, property)?;
    Ok(runner)
}

/// `min..max_exclusive` の長さを引く。
///
/// 範囲に含まれる下限、下限の次、上限の直前を境界として `BOUNDARY_RATIO` の割合で選ぶ。
/// 空・1 要素・上限付近が、一様分布だけだと薄いときに使う。
pub fn sample_len(ctx: &mut noprop::TestCaseContext, min: usize, max_exclusive: usize) -> usize {
    assert!(min < max_exclusive, "長さの範囲が空です");
    if max_exclusive - min == 1 {
        return min;
    }
    let last = max_exclusive - 1;
    let mut boundaries = [min, min, last];
    let mut count = 1;
    if min + 1 < last {
        boundaries[count] = min + 1;
        count += 1;
    }
    if last != min {
        boundaries[count] = last;
        count += 1;
    }
    noprop::sample_with_boundaries(ctx, &boundaries[..count], BOUNDARY_RATIO, |ctx| {
        noprop::sample_usize_in(ctx, min..max_exclusive)
    })
}

/// `min..=max_inclusive` の長さを引く。[`sample_len`] と同じ境界の付け方。
pub fn sample_len_inclusive(
    ctx: &mut noprop::TestCaseContext,
    min: usize,
    max_inclusive: usize,
) -> usize {
    assert!(min <= max_inclusive, "長さの範囲が空です");
    if max_inclusive == usize::MAX {
        return noprop::sample_usize_in(ctx, min..=max_inclusive);
    }
    sample_len(ctx, min, max_inclusive + 1)
}

/// 長さ `min..max_exclusive` のバイト列を引く。
pub fn sample_byte_vec(
    ctx: &mut noprop::TestCaseContext,
    min: usize,
    max_exclusive: usize,
) -> Vec<u8> {
    let len = sample_len(ctx, min, max_exclusive);
    noprop::sample_bytes_vec(ctx, len)
}

/// 文字数 `min_chars..=max_chars_inclusive` の Unicode 文字列を引く。
pub fn sample_text(
    ctx: &mut noprop::TestCaseContext,
    min_chars: usize,
    max_chars_inclusive: usize,
) -> String {
    let len = sample_len_inclusive(ctx, min_chars, max_chars_inclusive);
    noprop::sample_string(ctx, len)
}

/// `alphabet` から長さ `min_inclusive..=max_inclusive` の ASCII 文字列を引く。
///
/// `alphabet` の各バイトは ASCII であること。
pub fn sample_alphabet(
    ctx: &mut noprop::TestCaseContext,
    alphabet: &[u8],
    min_inclusive: usize,
    max_inclusive: usize,
) -> String {
    assert!(!alphabet.is_empty(), "文字集合が空です");
    assert!(
        alphabet.iter().all(u8::is_ascii),
        "文字集合は ASCII に限ります"
    );
    let len = sample_len_inclusive(ctx, min_inclusive, max_inclusive);
    let mut text = String::with_capacity(len);
    for _ in 0..len {
        text.push(noprop::sample_choice(ctx, alphabet) as char);
    }
    text
}

/// 先頭 1 文字を `first` から、残りを `rest` から引く。
///
/// 残りは `rest_min_inclusive..=rest_max_inclusive` 文字。0 を含めれば先頭だけの文字列になる。
pub fn sample_prefixed(
    ctx: &mut noprop::TestCaseContext,
    first: &[u8],
    rest: &[u8],
    rest_min_inclusive: usize,
    rest_max_inclusive: usize,
) -> String {
    assert!(!first.is_empty(), "先頭の文字集合が空です");
    let mut text = String::new();
    text.push(noprop::sample_choice(ctx, first) as char);
    text.push_str(&sample_alphabet(
        ctx,
        rest,
        rest_min_inclusive,
        rest_max_inclusive,
    ));
    text
}

/// 印字可能 ASCII (` ` から `~`) を `min_inclusive..=max_inclusive` 文字引く。
pub fn sample_printable_ascii(
    ctx: &mut noprop::TestCaseContext,
    min_inclusive: usize,
    max_inclusive: usize,
) -> String {
    let len = sample_len_inclusive(ctx, min_inclusive, max_inclusive);
    noprop::sample_ascii_printable_string(ctx, len)
}

/// 同じ ASCII バイトを `min_inclusive..=max_inclusive` 回繰り返した文字列を引く。
pub fn sample_repeated_ascii(
    ctx: &mut noprop::TestCaseContext,
    byte: u8,
    min_inclusive: usize,
    max_inclusive: usize,
) -> String {
    assert!(byte.is_ascii(), "繰り返すバイトは ASCII に限ります");
    let len = sample_len_inclusive(ctx, min_inclusive, max_inclusive);
    std::iter::repeat_n(byte as char, len).collect()
}

/// 長さ `min..max_exclusive` の要素列を、要素ごとのサンプラーで引く。
pub fn sample_vec<T>(
    ctx: &mut noprop::TestCaseContext,
    min: usize,
    max_exclusive: usize,
    sample_one: impl Fn(&mut noprop::TestCaseContext) -> T,
) -> Vec<T> {
    let len = sample_len(ctx, min, max_exclusive);
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(sample_one(ctx));
    }
    values
}

/// 必ず不正な UTF-8 になるバイト列を引く。
///
/// `0xFF` は UTF-8 のどの並びに現れないので、ランダムなバイト列のどこかに 1 バイト挿入する。
/// 空の列にも挿入するため、結果は常に 1 バイト以上になる。
pub fn sample_invalid_utf8(ctx: &mut noprop::TestCaseContext) -> Vec<u8> {
    let mut data = sample_byte_vec(ctx, 0, 100);
    let pos = if data.is_empty() {
        0
    } else {
        noprop::sample_usize_in(ctx, 0..=data.len())
    };
    data.insert(pos, 0xFF);
    data
}

/// 互いに異なる `[u8; N]` を 2 つ引く。
///
/// 偶然一致したときは先頭バイトを反転する。`N` は 1 以上。
pub fn sample_distinct_bytes<const N: usize>(
    ctx: &mut noprop::TestCaseContext,
) -> ([u8; N], [u8; N]) {
    assert!(N > 0, "区別する配列の長さは 1 以上です");
    let first = noprop::sample_bytes::<N>(ctx);
    let mut second = noprop::sample_bytes::<N>(ctx);
    if first == second {
        second[0] ^= 0xFF;
    }
    (first, second)
}

/// 送信可能な Close コードを引く。
///
/// 分岐は等重みで、`1000..=1003` / `1007..=1011` / `3000..=3999` / `4000..=4999`。
/// 値の個数で均等にすると、狭い正常系が広い私用領域に埋もれる。
pub fn sample_sendable_close_code(ctx: &mut noprop::TestCaseContext) -> u16 {
    match noprop::sample_weighted_index(ctx, &[1, 1, 1, 1]) {
        0 => noprop::sample_u16_in(ctx, 1000..=1003),
        1 => noprop::sample_u16_in(ctx, 1007..=1011),
        2 => noprop::sample_u16_in(ctx, 3000..=3999),
        _ => noprop::sample_u16_in(ctx, 4000..=4999),
    }
}

/// フレームに載せられる Close コードを引く。
///
/// 分岐は等重みで、`1000..=1003` / `1007..=1011` / `3000..=4999`。
pub fn sample_frame_close_code(ctx: &mut noprop::TestCaseContext) -> u16 {
    match noprop::sample_weighted_index(ctx, &[1, 1, 1]) {
        0 => noprop::sample_u16_in(ctx, 1000..=1003),
        1 => noprop::sample_u16_in(ctx, 1007..=1011),
        _ => noprop::sample_u16_in(ctx, 3000..=4999),
    }
}
