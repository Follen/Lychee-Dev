//! Approved client observations from Go v3.2.1, commit 10b561db5706702cd42e047eaeed4ccb5e825351.
//! Delivery products select a current series; these baselines retain compatible
//! older series and exact source evidence without inventing runtime acceptance.
#![forbid(unsafe_code)]
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ClientBaseline {
    pub product: &'static str,
    pub product_code: &'static str,
    pub data_slot: &'static str,
    pub series: &'static str,
    pub interface: u32,
    pub source_commit: &'static str,
    pub toc: &'static str,
}

pub(crate) static VERIFIED_BASELINES: [ClientBaseline; 7] = [
    ClientBaseline {
        product: "retail",
        product_code: "wow",
        data_slot: "",
        series: "12.1.5",
        interface: 120105,
        source_commit: "18c6d677b605b312f3d813d498404631fb941977",
        toc: "Lychee Dev.toc",
    },
    ClientBaseline {
        product: "retail",
        product_code: "wow",
        data_slot: "",
        series: "12.1.0",
        interface: 120100,
        source_commit: "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b",
        toc: "Lychee Dev.toc",
    },
    ClientBaseline {
        product: "classic",
        product_code: "wow_classic",
        data_slot: "",
        series: "5.5.4",
        interface: 50504,
        source_commit: "1028c1e687f721ba9d3af14d1b12a5745e4227c7",
        toc: "Lychee Dev.toc",
    },
    ClientBaseline {
        product: "classic-era",
        product_code: "wow_classic_era",
        data_slot: "",
        series: "1.15.9",
        interface: 11509,
        source_commit: "8165d4cd6e48d606369336cc3a7977902310e81e",
        toc: "Lychee Dev.toc",
    },
    ClientBaseline {
        product: "anniversary",
        product_code: "wow_anniversary",
        data_slot: "",
        series: "2.5.6",
        interface: 20506,
        source_commit: "2cfec7c00f0f24762d5ca35a4beb1e93c0cde328",
        toc: "Lychee Dev.toc",
    },
    ClientBaseline {
        product: "titan",
        product_code: "wow_classic_titan",
        data_slot: "",
        series: "3.80.2",
        interface: 38002,
        source_commit: "825d29d3662b372f0bead725ee6abd339e4a77b5",
        toc: "Lychee Dev.toc",
    },
    ClientBaseline {
        product: "forever",
        product_code: "wow_forever",
        data_slot: "wow_classic_beta",
        series: "1.60.1",
        interface: 16001,
        source_commit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069",
        toc: "Lychee Dev.toc",
    },
];

/// Catalog consistency is keyed by product/build and product-code/build. The
/// same product may intentionally have more than one verified client series.
pub(crate) fn catalog_problems(baselines: &[ClientBaseline]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut products = BTreeSet::new();
    let mut codes = BTreeSet::new();
    for baseline in baselines {
        let product_build = format!("{}/{}", baseline.product, baseline.series);
        let code_build = format!("{}/{}", baseline.product_code, baseline.series);
        if baseline.product.is_empty() || !products.insert(product_build.clone()) {
            problems.push(format!("duplicate or empty product/build {product_build}"));
        }
        if baseline.product_code.is_empty() || !codes.insert(code_build.clone()) {
            problems.push(format!("duplicate or empty productCode/build {code_build}"));
        }
        if !crate::selection::valid_digest(baseline.source_commit, 40) {
            problems.push(format!("invalid sourceCommit for {}", baseline.product));
        }
        if baseline.interface == 0 {
            problems.push(format!("invalid interface for {}", baseline.product));
        }
        if !baseline.toc.ends_with(".toc") {
            problems.push(format!("invalid toc for {}", baseline.product));
        }
        if !super::identity_build(&format!("{}.0", baseline.series)) {
            problems.push(format!("invalid buildSeries for {}", baseline.product));
        }
    }
    if baselines.is_empty() {
        problems.push("empty baseline catalog".into());
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_accepts_multiple_verified_series_for_one_product() {
        assert!(catalog_problems(&VERIFIED_BASELINES).is_empty());
        assert_eq!(
            VERIFIED_BASELINES
                .iter()
                .filter(|b| b.product == "retail")
                .count(),
            2
        );
    }

    #[test]
    fn catalog_rejects_duplicate_product_build_and_code_build() {
        let duplicate = [VERIFIED_BASELINES[0], VERIFIED_BASELINES[0]];
        let problems = catalog_problems(&duplicate);
        assert_eq!(
            problems,
            [
                "duplicate or empty product/build retail/12.1.5",
                "duplicate or empty productCode/build wow/12.1.5"
            ]
        );
        let mut same_code = VERIFIED_BASELINES[0];
        same_code.product = "other";
        assert_eq!(
            catalog_problems(&[VERIFIED_BASELINES[0], same_code]),
            ["duplicate or empty productCode/build wow/12.1.5"]
        );
    }

    #[test]
    fn catalog_rejects_empty_or_invalid_observations() {
        assert_eq!(catalog_problems(&[]), ["empty baseline catalog"]);
        let mut invalid = VERIFIED_BASELINES[0];
        invalid.product = "";
        invalid.product_code = "";
        invalid.source_commit = "unverified";
        invalid.interface = 0;
        invalid.toc = "not-a-manifest";
        invalid.series = "unknown";
        assert_eq!(catalog_problems(&[invalid]).len(), 6);
    }
}
