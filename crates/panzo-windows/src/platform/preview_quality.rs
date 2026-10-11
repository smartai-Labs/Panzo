//! Preview-only spatial resolution; never serialized into a project or export options.
use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,Copy,Default,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="kebab-case")]
pub enum PreviewQuality { Quarter, Half, #[default] Original }
impl PreviewQuality {
    pub const fn divisor(self)->u32 {match self {Self::Quarter=>4,Self::Half=>2,Self::Original=>1}}
    pub const fn index(self)->usize {match self {Self::Quarter=>0,Self::Half=>1,Self::Original=>2}}
    pub const fn from_index(index:usize)->Self {match index {0=>Self::Quarter,1=>Self::Half,_=>Self::Original}}
    pub fn limit(self,width:u32,height:u32)->Option<u32> {(self!=Self::Original).then(||(width.max(height)/self.divisor()).max(2))}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ratios_are_linear_and_original_is_unbounded() {
        assert_eq!(PreviewQuality::Quarter.limit(2560,1440),Some(640));
        assert_eq!(PreviewQuality::Half.limit(2560,1440),Some(1280));
        assert_eq!(PreviewQuality::Half.limit(1080,1920),Some(960));
        assert_eq!(PreviewQuality::Original.limit(7680,4320),None);
    }
}
