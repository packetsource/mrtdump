use crate::*;

#[derive(Debug)]
pub struct Prefix {
    pub prefix: IpAddr,
    pub len: u8
}
impl FromStr for Prefix {
    type Err = ();
    fn from_str(s: &str) -> Result<Prefix, Self::Err> {
        let token_split = s.split("/").collect::<Vec<&str>>();
        if token_split.len() == 2 {
            match IpAddr::from_str(token_split.get(0).unwrap()) {
                Ok(prefix) => {
                    match token_split.get(1).unwrap().parse::<u8>() {
                        Ok(len) => Ok(Prefix { prefix, len }),
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

impl Display for Prefix {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.prefix, self.len)
    }
}