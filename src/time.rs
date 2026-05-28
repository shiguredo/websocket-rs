/// Sans I/O パターンで時間を外部から与えるためのタイムスタンプ型
///
/// ミリ秒単位の時刻を表す。WebSocket ではタイムアウト管理に使用する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(u64);

impl Timestamp {
    /// ミリ秒からタイムスタンプを生成する
    pub fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// タイムスタンプをミリ秒として取得する
    pub fn as_millis(&self) -> u64 {
        self.0
    }

    /// 2 つのタイムスタンプの差分をミリ秒で取得する
    pub fn saturating_sub(&self, other: Self) -> u64 {
        self.0.saturating_sub(other.0)
    }

    /// タイムスタンプにミリ秒を加算する
    pub fn add_millis(&self, millis: u64) -> Self {
        Self(self.0.saturating_add(millis))
    }
}

impl std::ops::Add<u64> for Timestamp {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0.saturating_add(rhs))
    }
}

impl std::ops::Sub for Timestamp {
    type Output = u64;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0.saturating_sub(rhs.0)
    }
}
