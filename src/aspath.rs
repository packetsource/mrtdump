use crate::*;

#[derive(Debug, PartialEq)]
pub struct AsPathSegment {
    pub ordered: bool,  // ordered==true is AS_SEQUENCE, otherwise AS_SET
    pub asns: Vec<u32>,
}

#[derive(Debug, PartialEq)]
pub struct AsPath {
    pub aspath_segments: Vec<AsPathSegment>
}

impl Display for AsPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> anyhow::Result<(), std::fmt::Error> {
        for segment in &self.aspath_segments {
            if ! segment.ordered {
                write!(f, "{{")?;
            };

            write!(f, "{}", segment.asns
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
                .join(" ")
            )?;

            if ! segment.ordered {
                write!(f, "}}")?;
            };
        }
        Ok(())
    }
}

impl AsPath {

    pub fn new() -> AsPath {
        AsPath {
            aspath_segments: vec![]
        }
    }
    pub fn parse<R: Read>(reader: &mut R) -> Result<AsPath> {
        let mut aspath_segments: Vec<AsPathSegment> = Vec::new();
        while let Ok(segment_type) = reader.read_u8() {
            if segment_type != 1 && segment_type != 2 {
                return Err(anyhow!("AS_PATH attribute: segment ({}) not valid (should be AS_SET(1) or AS_SEQUENCE(2)", segment_type))
            }
            let length = reader.read_u8()?;
            let mut asns: Vec<u32> = Vec::new();
            for _ in 0..length {
                asns.push(reader.read_u32::<BigEndian>()?);
            }
            aspath_segments.push(AsPathSegment { ordered: segment_type==2, asns });
        }
        Ok(AsPath { aspath_segments })
    }

    pub fn contains(&self, asn: u32) -> bool {
        for segment in &self.aspath_segments {
            if segment.asns.contains(&asn) {
                return true;
            }
        }
        false
    }

}

