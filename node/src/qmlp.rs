//! QMLP-V1: integer-only quantized two-layer MLP inference, the second AI
//! workload whose results may enter consensus
//! (`protocol/spec/quantized-mlp-v1.md`).
//!
//! Validators recompute every partition with exact integer arithmetic, so a
//! result is accepted only if it is byte-identical to the specification.
//! This is full recomputation of a small model, not a succinct proof, and it
//! does not make floating-point PyTorch or ONNX execution verifiable.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::mining::scheduler::WorkRange;

const MODEL_DOMAIN: &[u8] = b"AIVOBLOCK-QMLP-MODEL-V1\0";
const RULES_DOMAIN: &[u8] = b"AIVOBLOCK-QMLP-RULES-V1\0";
const RESULT_DOMAIN: &[u8] = b"AIVOBLOCK-QMLP-RESULT-V1\0";
pub const MAX_QMLP_INPUT: usize = 32;
pub const MAX_QMLP_HIDDEN: usize = 64;
pub const MAX_QMLP_OUTPUT: usize = 16;
/// Inputs, weights and hidden activations are int8 without -128.
pub const QMLP_INT8_LIMIT: i64 = 127;
const BIAS_LIMIT: i32 = 1 << 24;
const MAX_MULTIPLIER: u32 = (1 << 31) - 1;
const MAX_SHIFT: u32 = 40;

/// Validated QMLP-V1 parameters; weight matrices are row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QmlpModelV1 {
    pub d_in: u32,
    pub d_hidden: u32,
    pub d_out: u32,
    pub multiplier: u32,
    pub shift: u32,
    pub w1: Vec<i8>,
    pub b1: Vec<i32>,
    pub w2: Vec<i8>,
    pub b2: Vec<i32>,
}

fn dims_in_range(d_in: u32, d_hidden: u32, d_out: u32) -> bool {
    let within = |value: u32, max: usize| value != 0 && value as usize <= max;
    within(d_in, MAX_QMLP_INPUT)
        && within(d_hidden, MAX_QMLP_HIDDEN)
        && within(d_out, MAX_QMLP_OUTPUT)
}

fn json_u32(value: &Value, name: &str) -> Result<u32, String> {
    value[name]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("QMLP field {name} must be a u32"))
}

fn json_ints<T: TryFrom<i64>>(value: &Value, name: &str) -> Result<Vec<T>, String> {
    let items = value[name]
        .as_array()
        .ok_or_else(|| format!("QMLP field {name} must be an array"))?;
    items
        .iter()
        .map(|x| {
            x.as_i64()
                .and_then(|n| T::try_from(n).ok())
                .ok_or_else(|| format!("QMLP field {name} is out of range"))
        })
        .collect()
}

struct Cursor<'a> {
    raw: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(count).ok_or("QMLP field overflow")?;
        let bytes = self.raw.get(self.at..end).ok_or("Truncated QMLP model")?;
        self.at = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i8s(&mut self, count: usize) -> Result<Vec<i8>, String> {
        Ok(self.take(count)?.iter().map(|b| *b as i8).collect())
    }

    fn i32s(&mut self, count: usize) -> Result<Vec<i32>, String> {
        let bytes = self.take(count * 4)?;
        let values = bytes
            .chunks_exact(4)
            .map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(values)
    }
}

impl QmlpModelV1 {
    fn dims(&self) -> (usize, usize, usize) {
        (
            self.d_in as usize,
            self.d_hidden as usize,
            self.d_out as usize,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if !dims_in_range(self.d_in, self.d_hidden, self.d_out) {
            return Err("QMLP dimensions exceed verifier limits".into());
        }
        if !(1..=MAX_MULTIPLIER).contains(&self.multiplier)
            || !(1..=MAX_SHIFT).contains(&self.shift)
        {
            return Err("QMLP requantization parameters out of range".into());
        }
        let (d_in, d_hidden, d_out) = self.dims();
        if self.w1.len() != d_hidden * d_in
            || self.b1.len() != d_hidden
            || self.w2.len() != d_out * d_hidden
            || self.b2.len() != d_out
        {
            return Err("QMLP parameter shapes do not match its dimensions".into());
        }
        let int8 = |w: &i8| *w != i8::MIN;
        let bias = |b: &i32| (-BIAS_LIMIT..=BIAS_LIMIT).contains(b);
        if !self.w1.iter().all(int8)
            || !self.w2.iter().all(int8)
            || !self.b1.iter().all(bias)
            || !self.b2.iter().all(bias)
        {
            return Err("QMLP weights or biases out of range".into());
        }
        Ok(())
    }

    /// Canonical `model_body` from the specification.
    pub fn encode_body(&self) -> Vec<u8> {
        let mut raw = Vec::new();
        for value in [
            self.d_in,
            self.d_hidden,
            self.d_out,
            self.multiplier,
            self.shift,
        ] {
            raw.extend_from_slice(&value.to_be_bytes());
        }
        raw.extend(self.w1.iter().map(|w| *w as u8));
        for bias in &self.b1 {
            raw.extend_from_slice(&bias.to_be_bytes());
        }
        raw.extend(self.w2.iter().map(|w| *w as u8));
        for bias in &self.b2 {
            raw.extend_from_slice(&bias.to_be_bytes());
        }
        raw
    }

    /// Decode and validate a model body at the front of `raw`. Returns the
    /// model and the number of bytes it occupied.
    pub fn decode_body(raw: &[u8]) -> Result<(Self, usize), String> {
        let mut cursor = Cursor { raw, at: 0 };
        let d_in = cursor.u32()?;
        let d_hidden = cursor.u32()?;
        let d_out = cursor.u32()?;
        let multiplier = cursor.u32()?;
        let shift = cursor.u32()?;
        // Bound every dimension before sizing a read.
        if !dims_in_range(d_in, d_hidden, d_out) {
            return Err("QMLP dimensions exceed verifier limits".into());
        }
        let (i, h, o) = (d_in as usize, d_hidden as usize, d_out as usize);
        let model = Self {
            d_in,
            d_hidden,
            d_out,
            multiplier,
            shift,
            w1: cursor.i8s(h * i)?,
            b1: cursor.i32s(h)?,
            w2: cursor.i8s(o * h)?,
            b2: cursor.i32s(o)?,
        };
        model.validate()?;
        Ok((model, cursor.at))
    }

    pub fn commitment(&self) -> Result<[u8; 32], String> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(MODEL_DOMAIN);
        hash.update(self.encode_body());
        Ok(hash.finalize().into())
    }

    /// Parameters in the Python worker's JSON shape.
    pub fn to_json(&self) -> Value {
        json!({
            "d_in": self.d_in,
            "d_hidden": self.d_hidden,
            "d_out": self.d_out,
            "multiplier": self.multiplier,
            "shift": self.shift,
            "w1": self.w1,
            "b1": self.b1,
            "w2": self.w2,
            "b2": self.b2,
        })
    }

    /// Parse and validate the Python worker's JSON shape (e.g. a model
    /// quantized off-chain from PyTorch).
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let model = Self {
            d_in: json_u32(value, "d_in")?,
            d_hidden: json_u32(value, "d_hidden")?,
            d_out: json_u32(value, "d_out")?,
            multiplier: json_u32(value, "multiplier")?,
            shift: json_u32(value, "shift")?,
            w1: json_ints(value, "w1")?,
            b1: json_ints(value, "b1")?,
            w2: json_ints(value, "w2")?,
            b2: json_ints(value, "b2")?,
        };
        model.validate()?;
        Ok(model)
    }

    /// Check one input row: exactly `d_in` values in the int8 range.
    pub fn check_row(&self, row: &[i64]) -> Result<(), String> {
        let limit = -QMLP_INT8_LIMIT..=QMLP_INT8_LIMIT;
        if row.len() != self.d_in as usize || !row.iter().all(|x| limit.contains(x)) {
            return Err("QMLP input row has the wrong width or is out of the int8 range".into());
        }
        Ok(())
    }

    /// Exact first layer: accumulate, requantize (round half up via a floor
    /// shift), fused ReLU and int8 clamp. The model must be validated.
    fn hidden(&self, row: &[i64]) -> Vec<i64> {
        let (d_in, _, _) = self.dims();
        let half = 1i64 << (self.shift - 1);
        let multiplier = i64::from(self.multiplier);
        self.w1
            .chunks_exact(d_in)
            .zip(&self.b1)
            .map(|(weights, bias)| {
                let products: i64 = weights
                    .iter()
                    .zip(row)
                    .map(|(w, x)| i64::from(*w) * x)
                    .sum();
                // |acc| < 2^25 and multiplier < 2^31, so this cannot overflow.
                let acc = i64::from(*bias) + products;
                let q = (acc * multiplier + half) >> self.shift;
                q.clamp(0, QMLP_INT8_LIMIT)
            })
            .collect()
    }

    fn forward(&self, row: &[i64]) -> Vec<i64> {
        let (_, d_hidden, _) = self.dims();
        let hidden = self.hidden(row);
        self.w2
            .chunks_exact(d_hidden)
            .zip(&self.b2)
            .map(|(weights, bias)| {
                let products: i64 = weights
                    .iter()
                    .zip(&hidden)
                    .map(|(w, h)| i64::from(*w) * h)
                    .sum();
                i64::from(*bias) + products
            })
            .collect()
    }

    /// Validated logits for one row.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn logits(&self, row: &[i64]) -> Result<Vec<i64>, String> {
        self.validate()?;
        self.check_row(row)?;
        Ok(self.forward(row))
    }
}

pub fn qmlp_rules_commitment_v1() -> [u8; 32] {
    Sha256::digest(RULES_DOMAIN).into()
}

/// Independent recomputation of one partition's committed logits.
pub fn qmlp_result_commitment_v1(
    model: &QmlpModelV1,
    rows: &[Vec<i64>],
    range: WorkRange,
) -> Result<[u8; 32], String> {
    model.validate()?;
    let start = usize::try_from(range.start).map_err(|_| "Range start overflow")?;
    let count = usize::try_from(range.count).map_err(|_| "Range count overflow")?;
    let end = start.checked_add(count).ok_or("Range overflow")?;
    if count == 0 || end > rows.len() {
        return Err("Claim exceeds signed task bounds".into());
    }
    let mut hash = Sha256::new();
    hash.update(RESULT_DOMAIN);
    hash.update(range.start.to_be_bytes());
    hash.update(range.count.to_be_bytes());
    hash.update(model.d_out.to_be_bytes());
    for row in &rows[start..end] {
        model.check_row(row)?;
        for logit in model.forward(row) {
            hash.update(logit.to_be_bytes());
        }
    }
    Ok(hash.finalize().into())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The shared cross-language vector (spec, `ai-engine/tests/test_qmlp.py`).
    pub fn fixture_model() -> QmlpModelV1 {
        QmlpModelV1 {
            d_in: 2,
            d_hidden: 2,
            d_out: 2,
            multiplier: 3,
            shift: 2,
            w1: vec![1, -2, 3, 4],
            b1: vec![0, -5],
            w2: vec![2, -1, -3, 5],
            b2: vec![1, 0],
        }
    }

    pub fn fixture_rows() -> Vec<Vec<i64>> {
        vec![vec![1, 2], vec![3, -4], vec![-2, 5], vec![127, 127]]
    }

    #[test]
    fn fixture_matches_the_cross_language_vector() {
        let model = fixture_model();
        let rows = fixture_rows();
        let hidden: Vec<Vec<i64>> = rows.iter().map(|row| model.hidden(row)).collect();
        assert_eq!(
            hidden,
            vec![vec![0, 5], vec![8, 0], vec![0, 7], vec![0, 127]]
        );
        let logits: Vec<Vec<i64>> = rows.iter().map(|row| model.logits(row).unwrap()).collect();
        assert_eq!(
            logits,
            vec![vec![-4, 25], vec![17, -24], vec![-6, 35], vec![-126, 635]]
        );
        assert_eq!(
            hex(&model.encode_body()),
            "000000020000000200000002000000030000000201fe030400000000fffffffb02fffd050000000100000000"
        );
        assert_eq!(
            hex(&model.commitment().unwrap()),
            "621e6ea400370f407c996e0aa2d5e96cf24a160a240d8c84f2bdc04b00f312e9"
        );
        assert_eq!(
            hex(&qmlp_rules_commitment_v1()),
            "0dcdcf5a65f06c5ca3c60aa419867528ad0d5a4bfaf38c4610012b97a11642db"
        );
        let all = qmlp_result_commitment_v1(&model, &rows, WorkRange { start: 0, count: 4 });
        assert_eq!(
            hex(&all.unwrap()),
            "b864ae656be24b63335f81a5a3c1029ca2c906edff09de387513977b2eca9b05"
        );
        let part = qmlp_result_commitment_v1(&model, &rows, WorkRange { start: 1, count: 2 });
        assert_eq!(
            hex(&part.unwrap()),
            "8ae5014a20a935bb5920ec1ea4f06ca0ab621403722faff0bf0822d52a2c0f87"
        );
    }

    /// CI "PyTorch / ONNX off-chain" job: a model quantized from PyTorch and
    /// its Python reference logits are reproduced bit-exactly by validators.
    #[test]
    fn exported_pytorch_qmlp_model_matches_rust_when_provided() {
        let Some(path) = std::env::var_os("AIVOBLOCK_QMLP_FILE") else {
            eprintln!("SKIP: AIVOBLOCK_QMLP_FILE is not set");
            return;
        };
        let raw = std::fs::read(&path).unwrap();
        let export: Value = serde_json::from_slice(&raw).unwrap();
        let model = QmlpModelV1::from_json(&export["model"]).unwrap();
        let model_commitment = hex(&model.commitment().unwrap());
        assert_eq!(model_commitment, export["model_commitment"]);
        let rows: Vec<Vec<i64>> = serde_json::from_value(export["rows"].clone()).unwrap();
        let logits: Vec<Vec<i64>> = rows.iter().map(|row| model.logits(row).unwrap()).collect();
        let expected: Vec<Vec<i64>> = serde_json::from_value(export["logits"].clone()).unwrap();
        assert_eq!(logits, expected);
        let range = WorkRange {
            start: 0,
            count: rows.len() as u64,
        };
        let commitment = qmlp_result_commitment_v1(&model, &rows, range).unwrap();
        assert_eq!(hex(&commitment), export["result_commitment"]);
        println!(
            "Rust reproduced {} PyTorch-quantized rows exactly; model {model_commitment}",
            rows.len()
        );
    }

    /// Timed Rust inference on the same *trained* model and exact input rows
    /// used by the Python service control. No blockchain or finality included.
    #[test]
    fn exported_qmlp_fixture_rust_timing_when_provided() {
        let Some(path) = std::env::var_os("AIVOBLOCK_SERVICE_FIXTURE") else {
            eprintln!("SKIP: AIVOBLOCK_SERVICE_FIXTURE is not set");
            return;
        };
        let export: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let model = QmlpModelV1::from_json(&export["model"]).unwrap();
        let rows: Vec<Vec<i64>> = serde_json::from_value(export["rows"].clone()).unwrap();
        assert!(!rows.is_empty() && rows.len() <= 256);
        assert_eq!(
            hex(&model.commitment().unwrap()),
            export["model_commitment"]
        );
        let inputs = crate::ai_block_v3::input_commitment_v3(&rows, model.d_in as usize).unwrap();
        assert_eq!(hex(&inputs), export["input_commitment"]);
        let result = qmlp_result_commitment_v1(
            &model,
            &rows,
            WorkRange {
                start: 0,
                count: rows.len() as u64,
            },
        )
        .unwrap();
        assert_eq!(hex(&result), export["result_commitment"]);

        // This timing includes the same public Rust API's per-row validation,
        // but not JSON deserialization, network, signatures or consensus.
        for row in &rows {
            std::hint::black_box(model.logits(row).unwrap());
        }
        let mut observations = Vec::new();
        for _ in 0..20 {
            let started = std::time::Instant::now();
            for row in &rows {
                std::hint::black_box(model.logits(std::hint::black_box(row)).unwrap());
            }
            observations.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        let output = json!({
            "schema_version": 1,
            "mode": "rust_reference_inference_only",
            "rows": rows.len(),
            "iterations": observations.len(),
            "model_commitment": export["model_commitment"],
            "input_commitment": export["input_commitment"],
            "result_commitment": export["result_commitment"],
            "latency_ms": { "observations": observations },
            "scope": "Rust CPU logits API only, no network or finality"
        });
        let path = std::env::var("AIVOBLOCK_RUST_SERVICE_REPORT")
            .expect("Set AIVOBLOCK_RUST_SERVICE_REPORT for the research artifact");
        std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }

    #[test]
    fn requantization_rounds_half_up_relu_clamps_and_cannot_overflow() {
        let single = |weight: i8, bias: i32, multiplier: u32, shift: u32| QmlpModelV1 {
            d_in: 1,
            d_hidden: 1,
            d_out: 1,
            multiplier,
            shift,
            w1: vec![weight],
            b1: vec![bias],
            w2: vec![1],
            b2: vec![0],
        };
        let half = single(1, 0, 1, 1);
        let outputs: Vec<i64> = [0, 1, 2, 3, 127, -1]
            .iter()
            .map(|x| half.logits(&[*x]).unwrap()[0])
            .collect();
        assert_eq!(outputs, vec![0, 1, 1, 2, 64, 0]);
        // Largest accumulator times the largest multiplier still fits in i64.
        let big = single(127, 1 << 24, (1 << 31) - 1, 1);
        assert_eq!(big.logits(&[127]).unwrap(), vec![127]);
        let negative = single(-127, -(1 << 24), (1 << 31) - 1, 40);
        assert_eq!(negative.logits(&[127]).unwrap(), vec![0]);
    }

    #[test]
    fn rejects_out_of_range_models_rows_and_ranges() {
        let base = fixture_model();
        let mut variants = Vec::new();
        for (d_in, d_hidden, d_out) in [(0, 2, 2), (33, 2, 2), (2, 65, 2), (2, 2, 17)] {
            variants.push(QmlpModelV1 {
                d_in,
                d_hidden,
                d_out,
                ..base.clone()
            });
        }
        for (multiplier, shift) in [(0, 2), (1 << 31, 2), (3, 0), (3, 41)] {
            variants.push(QmlpModelV1 {
                multiplier,
                shift,
                ..base.clone()
            });
        }
        let mut weight = base.clone();
        weight.w1[0] = i8::MIN;
        let mut bias = base.clone();
        bias.b2[1] = (1 << 24) + 1;
        let mut shape = base.clone();
        shape.w2.pop();
        variants.extend([weight, bias, shape]);
        for model in variants {
            assert!(model.validate().is_err(), "{model:?}");
            assert!(model.commitment().is_err());
        }
        for row in [vec![128, 0], vec![0, -128], vec![1], vec![1, 2, 3]] {
            assert!(base.logits(&row).is_err());
        }
        let rows = fixture_rows();
        for (start, count) in [(3, 2), (0, 0), (5, 1), (u64::MAX, 2)] {
            let range = WorkRange { start, count };
            assert!(qmlp_result_commitment_v1(&base, &rows, range).is_err());
        }
    }

    #[test]
    fn model_body_round_trips_and_rejects_truncation_and_bad_values() {
        let model = fixture_model();
        let mut raw = model.encode_body();
        let (decoded, used) = QmlpModelV1::decode_body(&raw).unwrap();
        assert_eq!((decoded, used), (model.clone(), raw.len()));
        // Trailing bytes are left for the enclosing task decoder.
        raw.extend_from_slice(&[9, 9]);
        assert_eq!(QmlpModelV1::decode_body(&raw).unwrap().1, raw.len() - 2);
        raw.truncate(raw.len() - 3);
        assert!(QmlpModelV1::decode_body(&raw).is_err());
        let mut huge = model.encode_body();
        huge[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(QmlpModelV1::decode_body(&huge).is_err());
        let mut minimum = model.encode_body();
        minimum[20] = 0x80; // w1[0] = -128
        assert!(QmlpModelV1::decode_body(&minimum).is_err());
        assert_eq!(QmlpModelV1::from_json(&model.to_json()).unwrap(), model);
        let mut json = model.to_json();
        json["w1"][0] = json!(300);
        assert!(QmlpModelV1::from_json(&json).is_err());
    }
}
