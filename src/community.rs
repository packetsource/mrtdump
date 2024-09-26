use crate::*;

#[derive(Debug, PartialEq)]
pub enum Community {
    Standard((u16, u16)),
    Large((u32, u32, u32))  // Not quite supported yet in attribute parsing
}
impl FromStr for Community {
    type Err = ();
    fn from_str(s: &str) -> Result<Community, Self::Err> {
        let token_split = s.split(":").collect::<Vec<&str>>();
        if token_split.len() == 2 {
            match token_split.get(0).unwrap().parse::<u16>() {
                Ok(a) => {
                    match token_split.get(1).unwrap().parse::<u16>() {
                        Ok(b) => Ok(Community::Standard((a, b))),
                        _ => Err(()),
                    }
                },
                _ => Err(()),
            }
        } else if token_split.len() == 3 {
            match token_split.get(0).unwrap().parse::<u32>() {
                Ok(a) => {
                    match token_split.get(1).unwrap().parse::<u32>() {
                        Ok(b) => {
                            match token_split.get(2).unwrap().parse::<u32>() {
                                Ok(c) => Ok(Community::Large((a, b, c))),
                                _ => Err(()),
                            }
                        },
                        _ => Err(()),
                    }
                },
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}

impl Display for Community {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Community::Standard((a, b)) => {
                write!(f, "{}:{}", a, b)
            },
            Community::Large((a, b, c)) => {
                write!(f, "{}:{}:{}", a, b, c)
            }
        }
    }
}

impl Community {
    pub fn parse<R: Read + BufRead>(reader: &mut R, num: usize) -> Result<Vec<Community>> {
        let mut community_list: Vec<Community> = vec![];
        for _ in 0..num {
            community_list.push(Community::Standard((reader.read_u16::<BigEndian>()?, reader.read_u16::<BigEndian>()?)))
        }
        Ok(community_list)
    }

}
