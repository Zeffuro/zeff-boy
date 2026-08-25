#[cfg(test)]
mod tests {
    use xdelta3::{decode, encode};

    #[test]
    fn basic_recoding() {
        let result = encode(&[1, 2, 3, 4, 5, 6, 7], &[1, 2, 4, 4, 7, 6, 7]);
        let recode = decode(result.unwrap().as_slice(), &[1, 2, 4, 4, 7, 6, 7]);
        assert_eq!(recode.unwrap().as_slice(), &[1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn larger_round_trip() {
        let source: Vec<u8> = (0..32 * 1024).map(|index| (index * 31) as u8).collect();
        let mut target = source.clone();
        target[1024..2048].fill(0xA5);
        target.extend_from_slice(b"tracked xdelta source");

        let patch = encode(&target, &source).unwrap();
        assert_eq!(decode(&patch, &source).unwrap(), target);
    }
}
