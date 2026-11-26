//! MQL5 `matrix` and `vector` types (double precision; `matrixf`/`vectorf` share them).
//!
//! Values are copied on assignment like other MQL5 value types; methods that change a
//! matrix (Init, Resize, Fill, CopyRates...) change it in place.

use super::*;
use crate::market::Timeframe;

pub type MatRef = Rc<RefCell<Mat>>;

#[derive(Debug, Clone, PartialEq)]
pub struct Mat {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
    /// a vector (one row, `cols` elements)
    pub vec: bool,
}

impl Mat {
    pub fn new(rows: usize, cols: usize) -> Mat {
        Mat {
            rows,
            cols,
            data: vec![0.0; rows * cols],
            vec: false,
        }
    }
    pub fn vector(data: Vec<f64>) -> Mat {
        Mat {
            rows: 1,
            cols: data.len(),
            data,
            vec: true,
        }
    }
    pub fn empty(vec: bool) -> Mat {
        Mat {
            rows: if vec { 1 } else { 0 },
            cols: 0,
            data: vec![],
            vec,
        }
    }
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.data[r * self.cols + c]
    }
    pub fn set(&mut self, r: usize, c: usize, v: f64) {
        self.data[r * self.cols + c] = v;
    }
    pub fn size(&self) -> usize {
        self.data.len()
    }
    fn map(&self, f: impl Fn(f64) -> f64) -> Mat {
        Mat {
            data: self.data.iter().map(|x| f(*x)).collect(),
            ..self.clone()
        }
    }
    pub fn identity(rows: usize, cols: usize, k: i64) -> Mat {
        let mut m = Mat::new(rows, cols);
        for r in 0..rows {
            let c = r as i64 + k;
            if c >= 0 && (c as usize) < cols {
                m.set(r, c as usize, 1.0);
            }
        }
        m
    }
    pub fn transpose(&self) -> Mat {
        if self.vec {
            return self.clone();
        }
        let mut t = Mat::new(self.cols, self.rows);
        for r in 0..self.rows {
            for c in 0..self.cols {
                t.set(c, r, self.at(r, c));
            }
        }
        t
    }
    pub fn row(&self, r: usize) -> Mat {
        Mat::vector(self.data[r * self.cols..(r + 1) * self.cols].to_vec())
    }
    pub fn col(&self, c: usize) -> Mat {
        Mat::vector((0..self.rows).map(|r| self.at(r, c)).collect())
    }
    /// Resize keeping the elements that still fit (row-major positions).
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let mut n = Mat::new(rows, cols);
        for r in 0..rows.min(self.rows) {
            for c in 0..cols.min(self.cols) {
                n.set(r, c, self.at(r, c));
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.data = n.data;
    }
    pub fn to_text(&self) -> String {
        let f = |v: &f64| fmt_double(*v);
        if self.vec {
            return format!(
                "[{}]",
                self.data.iter().map(f).collect::<Vec<_>>().join(",")
            );
        }
        let rows: Vec<String> = (0..self.rows)
            .map(|r| {
                format!(
                    "[{}]",
                    self.data[r * self.cols..(r + 1) * self.cols]
                        .iter()
                        .map(f)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect();
        format!("[{}]", rows.join("\n "))
    }
    pub fn matmul(&self, o: &Mat) -> Result<Mat, String> {
        // vector x matrix, matrix x vector, vector x vector (dot) and matrix x matrix
        match (self.vec, o.vec) {
            (true, true) => {
                if self.cols != o.cols {
                    return Err("vector sizes differ".into());
                }
                let d: f64 = self.data.iter().zip(&o.data).map(|(a, b)| a * b).sum();
                Ok(Mat::vector(vec![d]))
            }
            (true, false) => {
                if self.cols != o.rows {
                    return Err("vector size and matrix rows differ".into());
                }
                Ok(Mat::vector(
                    (0..o.cols)
                        .map(|c| (0..o.rows).map(|r| self.data[r] * o.at(r, c)).sum())
                        .collect(),
                ))
            }
            (false, true) => {
                if self.cols != o.cols {
                    return Err("matrix columns and vector size differ".into());
                }
                Ok(Mat::vector(
                    (0..self.rows)
                        .map(|r| (0..self.cols).map(|c| self.at(r, c) * o.data[c]).sum())
                        .collect(),
                ))
            }
            (false, false) => {
                if self.cols != o.rows {
                    return Err(format!(
                        "matrix sizes do not match for multiplication ({}x{} and {}x{})",
                        self.rows, self.cols, o.rows, o.cols
                    ));
                }
                let mut m = Mat::new(self.rows, o.cols);
                for r in 0..self.rows {
                    for k in 0..self.cols {
                        let a = self.at(r, k);
                        if a == 0.0 {
                            continue;
                        }
                        for c in 0..o.cols {
                            m.data[r * o.cols + c] += a * o.at(k, c);
                        }
                    }
                }
                Ok(m)
            }
        }
    }
    /// LU decomposition with partial pivoting: (lu, permutation, sign); None when singular.
    fn lu(&self) -> Option<(Mat, Vec<usize>, f64)> {
        let n = self.rows;
        if n != self.cols {
            return None;
        }
        let mut a = self.clone();
        let mut perm: Vec<usize> = (0..n).collect();
        let mut sign = 1.0;
        for k in 0..n {
            let p = (k..n).max_by(|&i, &j| a.at(i, k).abs().total_cmp(&a.at(j, k).abs()))?;
            if a.at(p, k).abs() < 1e-300 {
                return None;
            }
            if p != k {
                for c in 0..n {
                    let t = a.at(k, c);
                    a.set(k, c, a.at(p, c));
                    a.set(p, c, t);
                }
                perm.swap(k, p);
                sign = -sign;
            }
            for i in k + 1..n {
                let f = a.at(i, k) / a.at(k, k);
                a.set(i, k, f);
                for c in k + 1..n {
                    let v = a.at(i, c) - f * a.at(k, c);
                    a.set(i, c, v);
                }
            }
        }
        Some((a, perm, sign))
    }
    pub fn det(&self) -> f64 {
        match self.lu() {
            Some((lu, _, s)) => (0..self.rows).map(|i| lu.at(i, i)).product::<f64>() * s,
            None => 0.0,
        }
    }
    /// Solve A x = b for each column of b.
    pub fn solve(&self, b: &Mat) -> Option<Mat> {
        let n = self.rows;
        let (lu, perm, _) = self.lu()?;
        let bm = if b.vec {
            Mat {
                rows: b.cols,
                cols: 1,
                data: b.data.clone(),
                vec: false,
            }
        } else {
            b.clone()
        };
        if bm.rows != n {
            return None;
        }
        let mut x = Mat::new(n, bm.cols);
        for c in 0..bm.cols {
            let mut y = vec![0.0; n];
            for i in 0..n {
                let mut s = bm.at(perm[i], c);
                for k in 0..i {
                    s -= lu.at(i, k) * y[k];
                }
                y[i] = s;
            }
            for i in (0..n).rev() {
                let mut s = y[i];
                for k in i + 1..n {
                    s -= lu.at(i, k) * x.at(k, c);
                }
                x.set(i, c, s / lu.at(i, i));
            }
        }
        if b.vec {
            Some(Mat::vector(x.data))
        } else {
            Some(x)
        }
    }
    pub fn inv(&self) -> Option<Mat> {
        self.solve(&Mat::identity(self.rows, self.rows, 0))
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}
fn var(v: &[f64], ddof: f64) -> f64 {
    let m = mean(v);
    let n = v.len() as f64 - ddof;
    if n <= 0.0 {
        return f64::NAN;
    }
    v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n
}
fn percentile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let pos = (q / 100.0).clamp(0.0, 1.0) * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}

fn argmax(v: &[f64], max: bool) -> usize {
    let mut best = 0;
    for (i, x) in v.iter().enumerate() {
        if (max && *x > v[best]) || (!max && *x < v[best]) {
            best = i;
        }
    }
    best
}

/// Values reduced along an axis: None = all elements, 0 = down the rows (one result per
/// column), 1 = along each row.
fn reduce(m: &Mat, axis: Option<i64>, f: impl Fn(&[f64]) -> f64) -> Value {
    match axis {
        Some(0) if !m.vec => Value::Mat(new_mat(Mat::vector(
            (0..m.cols).map(|c| f(&m.col(c).data)).collect(),
        ))),
        Some(1) if !m.vec => Value::Mat(new_mat(Mat::vector(
            (0..m.rows).map(|r| f(&m.row(r).data)).collect(),
        ))),
        _ => Value::Double(f(&m.data)),
    }
}

pub fn new_mat(m: Mat) -> MatRef {
    Rc::new(RefCell::new(m))
}

/// Numbers from a value: a matrix/vector, an array (also nested) or a single number.
fn flatten(v: &Value, out: &mut Vec<f64>) {
    match v {
        Value::Mat(m) => out.extend(m.borrow().data.iter().copied()),
        Value::Arr(a) => {
            for x in a.borrow().data.iter() {
                flatten(x, out);
            }
        }
        other => out.push(other.as_f64()),
    }
}

/// Build a matrix or vector from an initializer list value (`{1,2}` or `{{1,2},{3,4}}`).
pub fn from_value(v: &Value, want_vec: bool) -> Mat {
    if let Value::Mat(m) = v {
        let mut m = m.borrow().clone();
        if want_vec && !m.vec {
            m = Mat::vector(m.data);
        }
        return m;
    }
    let rows: Vec<Vec<f64>> = match v {
        Value::Arr(a) => {
            let a = a.borrow();
            if a.data
                .iter()
                .any(|x| matches!(x, Value::Arr(_) | Value::Mat(_)))
            {
                a.data
                    .iter()
                    .map(|x| {
                        let mut r = vec![];
                        flatten(x, &mut r);
                        r
                    })
                    .collect()
            } else {
                vec![a.data.iter().map(|x| x.as_f64()).collect()]
            }
        }
        other => vec![vec![other.as_f64()]],
    };
    if want_vec {
        return Mat::vector(rows.into_iter().flatten().collect());
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut m = Mat::new(rows.len(), cols);
    for (r, row) in rows.iter().enumerate() {
        for (c, x) in row.iter().enumerate() {
            m.set(r, c, *x);
        }
    }
    m
}

fn ai(a: &[Value], i: usize, d: i64) -> i64 {
    a.get(i).map(|v| v.as_i64()).unwrap_or(d)
}
fn af(a: &[Value], i: usize, d: f64) -> f64 {
    a.get(i).map(|v| v.as_f64()).unwrap_or(d)
}
fn vm(m: Mat) -> Value {
    Value::Mat(new_mat(m))
}

/// The element-wise math functions that accept a matrix or vector.
pub fn elementwise(name: &str, a: &[Value]) -> Option<Value> {
    let m = match a.first() {
        Some(Value::Mat(m)) => m.borrow().clone(),
        _ => return None,
    };
    let p = af(a, 1, 0.0);
    let f: Box<dyn Fn(f64) -> f64> = match name {
        "MathAbs" | "fabs" => Box::new(f64::abs),
        "MathSqrt" | "sqrt" => Box::new(f64::sqrt),
        "MathExp" | "exp" => Box::new(f64::exp),
        "MathLog" | "log" => Box::new(f64::ln),
        "MathLog10" | "log10" => Box::new(f64::log10),
        "MathLog2" => Box::new(f64::log2),
        "MathLog1p" => Box::new(f64::ln_1p),
        "MathExpm1" => Box::new(f64::exp_m1),
        "MathSin" | "sin" => Box::new(f64::sin),
        "MathCos" | "cos" => Box::new(f64::cos),
        "MathTan" | "tan" => Box::new(f64::tan),
        "MathArcsin" | "asin" => Box::new(f64::asin),
        "MathArccos" | "acos" => Box::new(f64::acos),
        "MathArctan" | "atan" => Box::new(f64::atan),
        "MathSinh" => Box::new(f64::sinh),
        "MathCosh" => Box::new(f64::cosh),
        "MathTanh" => Box::new(f64::tanh),
        "MathFloor" | "floor" => Box::new(f64::floor),
        "MathCeil" | "ceil" => Box::new(f64::ceil),
        "MathRound" | "round" => Box::new(f64::round),
        "MathSign" => Box::new(f64::signum),
        "MathPow" | "pow" => match a.get(1) {
            Some(Value::Mat(e)) => {
                let e = e.borrow();
                if e.size() != m.size() {
                    return None;
                }
                let data = m
                    .data
                    .iter()
                    .zip(&e.data)
                    .map(|(x, y)| x.powf(*y))
                    .collect();
                return Some(vm(Mat { data, ..m }));
            }
            _ => Box::new(move |x: f64| x.powf(p)),
        },
        "MathMod" | "fmod" => Box::new(move |x: f64| x % p),
        "MathMax" | "fmax" => Box::new(move |x: f64| x.max(p)),
        "MathMin" | "fmin" => Box::new(move |x: f64| x.min(p)),
        _ => return None,
    };
    Some(vm(m.map(f)))
}

/// `a op b` where at least one side is a matrix or vector.
pub fn binop(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    use BinOp::*;
    let ew = |x: f64, y: f64| -> f64 {
        match op {
            Add => x + y,
            Sub => x - y,
            Mul => x * y,
            Div => x / y,
            _ => x % y,
        }
    };
    match (l, r) {
        (Value::Mat(a), Value::Mat(b)) => {
            let (a, b) = (a.borrow(), b.borrow());
            match op {
                Eq | Ne => {
                    let same = a.rows == b.rows && a.cols == b.cols && a.data == b.data;
                    return Ok(Value::Bool(same == (op == Eq)));
                }
                Add | Sub | Mul | Div | Mod => {}
                _ => return Err("illegal operation with matrices".into()),
            }
            if a.data.len() != b.data.len() {
                return Err(format!(
                    "matrix/vector sizes differ ({} and {} elements)",
                    a.data.len(),
                    b.data.len()
                ));
            }
            let data = a
                .data
                .iter()
                .zip(&b.data)
                .map(|(x, y)| ew(*x, *y))
                .collect();
            Ok(vm(Mat { data, ..a.clone() }))
        }
        (Value::Mat(a), s) => {
            if !matches!(op, Add | Sub | Mul | Div | Mod) {
                return Err("illegal operation with matrices".into());
            }
            let y = s.as_f64();
            Ok(vm(a.borrow().map(|x| ew(x, y))))
        }
        (s, Value::Mat(b)) => {
            if !matches!(op, Add | Sub | Mul | Div | Mod) {
                return Err("illegal operation with matrices".into());
            }
            let x = s.as_f64();
            Ok(vm(b.borrow().map(|y| ew(x, y))))
        }
        _ => Err("matrix expected".into()),
    }
}

/// `matrix::Eye(...)`, `vector::Ones(...)` and the other static constructors; also the
/// declaration forms `matrix m(rows, cols)` and `vector v(size)`.
pub fn static_ctor(name: &str, a: &[Value]) -> Option<Value> {
    let (is_vec, f) = if let Some(f) = name.strip_prefix("matrix::") {
        (false, f)
    } else if let Some(f) = name.strip_prefix("vector::") {
        (true, f)
    } else {
        return None;
    };
    let rows = ai(a, 0, 0).max(0) as usize;
    let cols = ai(a, 1, 0).max(0) as usize;
    let shape = |fill: f64, val_at: usize| -> Mat {
        if is_vec {
            Mat::vector(vec![fill; rows])
        } else {
            let _ = val_at;
            let mut m = Mat::new(rows, cols);
            m.data.iter_mut().for_each(|x| *x = fill);
            m
        }
    };
    let m = match f {
        "__ctor" => shape(0.0, 0),
        "Zeros" => shape(0.0, 0),
        "Ones" => shape(1.0, 0),
        "Full" => {
            let v = if is_vec { af(a, 1, 0.0) } else { af(a, 2, 0.0) };
            shape(v, 0)
        }
        "Eye" => Mat::identity(rows, cols, ai(a, 2, 0)),
        "Identity" => Mat::identity(rows, cols, 0),
        "Tri" => {
            let k = ai(a, 2, 0);
            let mut m = Mat::new(rows, cols);
            for r in 0..rows {
                for c in 0..cols {
                    if (c as i64) <= r as i64 + k {
                        m.set(r, c, 1.0);
                    }
                }
            }
            m
        }
        "Random" => {
            let (lo, hi) = if is_vec {
                (af(a, 1, 0.0), af(a, 2, 1.0))
            } else {
                (af(a, 2, 0.0), af(a, 3, 1.0))
            };
            let mut rng = crate::market::Rng::new(crate::market::unix_now() as u64 ^ 0x5eed);
            let mut m = shape(0.0, 0);
            m.data
                .iter_mut()
                .for_each(|x| *x = lo + (hi - lo) * rng.uniform());
            m
        }
        _ => return None,
    };
    Some(vm(m))
}

/// Known method names (the semantic check uses them).
pub const METHODS: &[&str] = &[
    "Init",
    "Assign",
    "Resize",
    "Reshape",
    "Rows",
    "Cols",
    "Size",
    "Fill",
    "Zeros",
    "Ones",
    "Full",
    "Eye",
    "Identity",
    "Tri",
    "Random",
    "Transpose",
    "MatMul",
    "Dot",
    "Outer",
    "Kron",
    "Det",
    "Inv",
    "PInv",
    "Solve",
    "LstSq",
    "Trace",
    "Norm",
    "Rank",
    "Sum",
    "Prod",
    "Mean",
    "Average",
    "Median",
    "Max",
    "Min",
    "ArgMax",
    "ArgMin",
    "Std",
    "Var",
    "CumSum",
    "CumProd",
    "Ptp",
    "Percentile",
    "Quantile",
    "Col",
    "Row",
    "Diag",
    "Copy",
    "Swap",
    "Clip",
    "Sort",
    "ArgSort",
    "Compare",
    "CompareByDigits",
    "Flat",
    "HasNan",
    "CopyRates",
    "CopyIndicatorBuffer",
    "CorrCoef",
    "Cov",
    "Correlate",
    "Convolve",
    "Hsplit",
    "Vsplit",
    "SwapRows",
    "SwapCols",
    "Std",
    "LinearRegression",
    "Loss",
    "RegressionMetric",
];

impl Instance {
    pub(super) fn mat_method(
        &mut self,
        mr: &MatRef,
        name: &str,
        args: &[Expr],
        host: &mut dyn Host,
    ) -> R<Value> {
        let a = self.eval_args(args, host)?;
        let err = |s: &Instance, m: &str| Err(s.err(m.to_string()));
        let is_vec = mr.borrow().vec;
        // methods that change the matrix
        match name {
            "Init" | "Resize" => {
                let rows = ai(&a, 0, 0).max(0) as usize;
                let cols = if is_vec {
                    0
                } else {
                    ai(&a, 1, 0).max(0) as usize
                };
                let mut m = mr.borrow_mut();
                if is_vec {
                    let n = rows;
                    if name == "Init" {
                        *m = Mat::vector(vec![0.0; n]);
                    } else {
                        m.data.resize(n, 0.0);
                        m.cols = n;
                    }
                } else if name == "Init" {
                    *m = Mat::new(rows, cols);
                } else {
                    m.resize(rows, cols);
                }
                return Ok(Value::Bool(true));
            }
            "Reshape" => {
                let rows = ai(&a, 0, 0).max(0) as usize;
                let cols = ai(&a, 1, 0).max(0) as usize;
                let mut m = mr.borrow_mut();
                m.data.resize(rows * cols, 0.0);
                m.rows = rows;
                m.cols = cols;
                m.vec = false;
                return Ok(Value::Bool(true));
            }
            "Fill" => {
                let v = af(&a, 0, 0.0);
                mr.borrow_mut().data.iter_mut().for_each(|x| *x = v);
                return Ok(Value::Bool(true));
            }
            "Zeros" | "Ones" | "Full" | "Eye" | "Identity" | "Tri" | "Random" => {
                let pre = if is_vec { "vector::" } else { "matrix::" };
                if let Some(Value::Mat(n)) = static_ctor(&format!("{}{}", pre, name), &a) {
                    let n = n.borrow().clone();
                    *mr.borrow_mut() = n;
                }
                return Ok(Value::Bool(true));
            }
            "Assign" => {
                let src = a.first().cloned().unwrap_or(Value::Void);
                let n = from_value(&src, is_vec);
                *mr.borrow_mut() = n;
                return Ok(Value::Bool(true));
            }
            "Swap" => {
                // swaps with another matrix variable: values are references here
                if let Some(Value::Mat(o)) = a.first() {
                    if !Rc::ptr_eq(o, mr) {
                        std::mem::swap(&mut *mr.borrow_mut(), &mut *o.borrow_mut());
                    }
                }
                return Ok(Value::Bool(true));
            }
            "SwapRows" | "SwapCols" => {
                let (i, j) = (ai(&a, 0, 0) as usize, ai(&a, 1, 0) as usize);
                let mut m = mr.borrow_mut();
                if name == "SwapRows" && i < m.rows && j < m.rows {
                    for c in 0..m.cols {
                        let t = m.at(i, c);
                        let v = m.at(j, c);
                        m.set(i, c, v);
                        m.set(j, c, t);
                    }
                } else if name == "SwapCols" && i < m.cols && j < m.cols {
                    for r in 0..m.rows {
                        let t = m.at(r, i);
                        let v = m.at(r, j);
                        m.set(r, i, v);
                        m.set(r, j, t);
                    }
                }
                return Ok(Value::Bool(true));
            }
            "Clip" => {
                let (lo, hi) = (af(&a, 0, f64::MIN), af(&a, 1, f64::MAX));
                mr.borrow_mut()
                    .data
                    .iter_mut()
                    .for_each(|x| *x = x.clamp(lo, hi));
                return Ok(Value::Bool(true));
            }
            "Sort" => {
                let desc = ai(&a, 0, 0) != 0;
                let mut m = mr.borrow_mut();
                m.data
                    .sort_by(|x, y| if desc { y.total_cmp(x) } else { x.total_cmp(y) });
                return Ok(Value::Bool(true));
            }
            "CopyRates" => return self.mat_copy_rates(mr, &a, host),
            "CopyIndicatorBuffer" => {
                let h = ai(&a, 0, -1);
                let buf = ai(&a, 1, 0).max(0) as usize;
                let start = ai(&a, 2, 0).max(0) as usize;
                let count = ai(&a, 3, 0).max(0) as usize;
                let data = self.handle_buffer(host, h, buf)?;
                let Some(data) = data else {
                    return Ok(Value::Bool(false));
                };
                let end = data.len().saturating_sub(start);
                let from = end.saturating_sub(count);
                *mr.borrow_mut() = Mat::vector(data[from..end].to_vec());
                return Ok(Value::Bool(end > from));
            }
            "Col" | "Row" if a.len() >= 2 => {
                // set a column/row from a vector: m.Col(v, index)
                let src = from_value(&a[0], true);
                let k = ai(&a, 1, 0).max(0) as usize;
                let mut m = mr.borrow_mut();
                if name == "Col" && k < m.cols {
                    for r in 0..m.rows.min(src.size()) {
                        m.set(r, k, src.data[r]);
                    }
                } else if name == "Row" && k < m.rows {
                    for c in 0..m.cols.min(src.size()) {
                        m.set(k, c, src.data[c]);
                    }
                }
                return Ok(Value::Bool(true));
            }
            "Diag" if a.first().is_some_and(|v| matches!(v, Value::Mat(_))) => {
                let src = from_value(&a[0], true);
                let k = ai(&a, 1, 0);
                let n = src.size() + k.unsigned_abs() as usize;
                let mut m = Mat::new(n, n);
                for (i, v) in src.data.iter().enumerate() {
                    let (r, c) = if k >= 0 {
                        (i, i + k as usize)
                    } else {
                        (i + (-k) as usize, i)
                    };
                    m.set(r, c, *v);
                }
                *mr.borrow_mut() = m;
                return Ok(Value::Bool(true));
            }
            _ => {}
        }
        let m = mr.borrow().clone();
        let axis = a.first().map(|v| v.as_i64());
        Ok(match name {
            "Rows" => Value::Int(if m.vec { m.cols } else { m.rows } as i64),
            "Cols" => Value::Int(m.cols as i64),
            "Size" => Value::Int(m.size() as i64),
            "Copy" => vm(m),
            "Transpose" => vm(m.transpose()),
            "Flat" => Value::Double(
                m.data
                    .get(ai(&a, 0, 0).max(0) as usize)
                    .copied()
                    .unwrap_or(f64::NAN),
            ),
            "HasNan" => Value::Int(m.data.iter().filter(|x| x.is_nan()).count() as i64),
            "MatMul" | "Dot" => {
                let o = from_value(a.first().unwrap_or(&Value::Void), false);
                let o = match a.first() {
                    Some(Value::Mat(x)) => x.borrow().clone(),
                    _ => o,
                };
                match m.matmul(&o) {
                    Ok(r) if m.vec && o.vec => Value::Double(r.data[0]),
                    Ok(r) => vm(r),
                    Err(e) => return err(self, &e),
                }
            }
            "Outer" => {
                let o = from_value(a.first().unwrap_or(&Value::Void), true);
                let mut r = Mat::new(m.size(), o.size());
                for (i, x) in m.data.iter().enumerate() {
                    for (j, y) in o.data.iter().enumerate() {
                        r.set(i, j, x * y);
                    }
                }
                vm(r)
            }
            "Kron" => {
                let o = match a.first() {
                    Some(Value::Mat(x)) => x.borrow().clone(),
                    _ => return err(self, "Kron: matrix expected"),
                };
                let mut r = Mat::new(m.rows * o.rows, m.cols * o.cols);
                for i in 0..m.rows {
                    for j in 0..m.cols {
                        for k in 0..o.rows {
                            for l in 0..o.cols {
                                r.set(i * o.rows + k, j * o.cols + l, m.at(i, j) * o.at(k, l));
                            }
                        }
                    }
                }
                if m.vec && o.vec {
                    vm(Mat::vector(r.data))
                } else {
                    vm(r)
                }
            }
            "Det" => Value::Double(m.det()),
            "Inv" => match m.inv() {
                Some(i) => vm(i),
                None => vm(Mat::new(m.rows, m.cols).map(|_| f64::NAN)),
            },
            "PInv" => {
                // (AᵀA)⁻¹Aᵀ for full column rank, Aᵀ(AAᵀ)⁻¹ otherwise
                let t = m.transpose();
                let r = if m.rows >= m.cols {
                    t.matmul(&m)
                        .ok()
                        .and_then(|x| x.inv())
                        .and_then(|x| x.matmul(&t).ok())
                } else {
                    m.matmul(&t)
                        .ok()
                        .and_then(|x| x.inv())
                        .and_then(|x| t.matmul(&x).ok())
                };
                vm(r.unwrap_or_else(|| Mat::new(m.cols, m.rows).map(|_| f64::NAN)))
            }
            "Solve" => {
                let b = from_value(a.first().unwrap_or(&Value::Void), true);
                let b = match a.first() {
                    Some(Value::Mat(x)) => x.borrow().clone(),
                    _ => b,
                };
                match m.solve(&b) {
                    Some(x) => vm(x),
                    None => return err(self, "Solve: the matrix is singular"),
                }
            }
            "LstSq" => {
                let b = match a.first() {
                    Some(Value::Mat(x)) => x.borrow().clone(),
                    _ => return err(self, "LstSq: matrix or vector expected"),
                };
                let t = m.transpose();
                let ata = t.matmul(&m).map_err(|e| self.err(e))?;
                let atb = t.matmul(&b).map_err(|e| self.err(e))?;
                match ata.solve(&atb) {
                    Some(x) => vm(x),
                    None => return err(self, "LstSq: no unique solution"),
                }
            }
            "Trace" => Value::Double((0..m.rows.min(m.cols)).map(|i| m.at(i, i)).sum()),
            "Rank" => {
                // Gaussian elimination
                let mut x = m.clone();
                let mut rank = 0;
                let mut row = 0;
                for c in 0..x.cols {
                    if row >= x.rows {
                        break;
                    }
                    let p = (row..x.rows)
                        .max_by(|&i, &j| x.at(i, c).abs().total_cmp(&x.at(j, c).abs()));
                    let Some(p) = p else { break };
                    if x.at(p, c).abs() < 1e-10 {
                        continue;
                    }
                    for k in 0..x.cols {
                        let t = x.at(row, k);
                        let v = x.at(p, k);
                        x.set(row, k, v);
                        x.set(p, k, t);
                    }
                    for i in row + 1..x.rows {
                        let f = x.at(i, c) / x.at(row, c);
                        for k in c..x.cols {
                            let v = x.at(i, k) - f * x.at(row, k);
                            x.set(i, k, v);
                        }
                    }
                    row += 1;
                    rank += 1;
                }
                Value::Int(rank)
            }
            "Norm" => {
                // VECTOR_NORM_INF=0, MINUS_INF=1, P=2 ; MATRIX_NORM_FROBENIUS etc.
                let kind = ai(&a, 0, 2);
                let p = af(&a, 1, 2.0);
                let d = &m.data;
                Value::Double(if m.vec {
                    match kind {
                        0 => d.iter().fold(0.0f64, |s, x| s.max(x.abs())),
                        1 => d.iter().fold(f64::MAX, |s, x| s.min(x.abs())),
                        _ => d.iter().map(|x| x.abs().powf(p)).sum::<f64>().powf(1.0 / p),
                    }
                } else {
                    match kind {
                        // MATRIX_NORM_P1 (max column sum)
                        2 => (0..m.cols)
                            .map(|c| m.col(c).data.iter().map(|x| x.abs()).sum::<f64>())
                            .fold(0.0, f64::max),
                        // MATRIX_NORM_INF (max row sum)
                        4 => (0..m.rows)
                            .map(|r| m.row(r).data.iter().map(|x| x.abs()).sum::<f64>())
                            .fold(0.0, f64::max),
                        _ => d.iter().map(|x| x * x).sum::<f64>().sqrt(),
                    }
                })
            }
            "Sum" => reduce(&m, axis, |v| v.iter().sum()),
            "Prod" => reduce(&m, axis, |v| v.iter().product()),
            "Mean" | "Average" => reduce(&m, axis, mean),
            "Median" => reduce(&m, axis, |v| percentile(v, 50.0)),
            "Max" => reduce(&m, axis, |v| v.iter().copied().fold(f64::MIN, f64::max)),
            "Min" => reduce(&m, axis, |v| v.iter().copied().fold(f64::MAX, f64::min)),
            "Ptp" => reduce(&m, axis, |v| {
                v.iter().copied().fold(f64::MIN, f64::max)
                    - v.iter().copied().fold(f64::MAX, f64::min)
            }),
            "ArgMax" => match reduce(&m, axis, |v| argmax(v, true) as f64) {
                Value::Double(d) => Value::Int(d as i64),
                other => other,
            },
            "ArgMin" => match reduce(&m, axis, |v| argmax(v, false) as f64) {
                Value::Double(d) => Value::Int(d as i64),
                other => other,
            },
            "Std" => {
                let ddof = af(&a, 1, 0.0);
                reduce(&m, axis, |v| var(v, ddof).sqrt())
            }
            "Var" => {
                let ddof = af(&a, 1, 0.0);
                reduce(&m, axis, |v| var(v, ddof))
            }
            "Percentile" | "Quantile" => {
                let q = af(&a, 0, 50.0) * if name == "Quantile" { 100.0 } else { 1.0 };
                let ax = a.get(1).map(|v| v.as_i64());
                reduce(&m, ax, |v| percentile(v, q))
            }
            "CumSum" | "CumProd" => {
                let mut acc = if name == "CumSum" { 0.0 } else { 1.0 };
                let data = m
                    .data
                    .iter()
                    .map(|x| {
                        if name == "CumSum" {
                            acc += x
                        } else {
                            acc *= x
                        }
                        acc
                    })
                    .collect();
                if m.vec {
                    vm(Mat { data, ..m })
                } else {
                    vm(Mat::vector(data))
                }
            }
            "Col" => {
                let k = ai(&a, 0, 0).max(0) as usize;
                if k >= m.cols {
                    return err(self, "matrix column out of range");
                }
                vm(m.col(k))
            }
            "Row" => {
                let k = ai(&a, 0, 0).max(0) as usize;
                if k >= m.rows {
                    return err(self, "matrix row out of range");
                }
                vm(m.row(k))
            }
            "Diag" => {
                let k = ai(&a, 0, 0);
                let d = (0..m.rows)
                    .filter_map(|r| {
                        let c = r as i64 + k;
                        (c >= 0 && (c as usize) < m.cols).then(|| m.at(r, c as usize))
                    })
                    .collect();
                vm(Mat::vector(d))
            }
            "ArgSort" => {
                let mut idx: Vec<usize> = (0..m.size()).collect();
                idx.sort_by(|x, y| m.data[*x].total_cmp(&m.data[*y]));
                vm(Mat::vector(idx.into_iter().map(|i| i as f64).collect()))
            }
            "Compare" | "CompareByDigits" => {
                let o = from_value(a.first().unwrap_or(&Value::Void), m.vec);
                if o.size() != m.size() {
                    return Ok(Value::Int(-1));
                }
                let eps = if name == "Compare" {
                    af(&a, 1, 0.0)
                } else {
                    0.5 * 10f64.powi(-(ai(&a, 1, 8) as i32))
                };
                let mut c = 0i64;
                for (x, y) in m.data.iter().zip(&o.data) {
                    if (x - y).abs() > eps {
                        c = if x < y { -1 } else { 1 };
                        break;
                    }
                }
                Value::Int(c)
            }
            "CorrCoef" => {
                let o = from_value(a.first().unwrap_or(&Value::Void), true);
                if m.vec {
                    let (x, y) = (&m.data, &o.data);
                    let n = x.len().min(y.len());
                    let (mx, my) = (mean(&x[..n]), mean(&y[..n]));
                    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
                    for i in 0..n {
                        sxy += (x[i] - mx) * (y[i] - my);
                        sxx += (x[i] - mx).powi(2);
                        syy += (y[i] - my).powi(2);
                    }
                    Value::Double(sxy / (sxx * syy).sqrt())
                } else {
                    // correlation between rows (rowvar=true) or columns
                    let rowvar = a.first().map(|v| v.truthy()).unwrap_or(true);
                    let src = if rowvar { m.clone() } else { m.transpose() };
                    let cov = covariance(&src);
                    let mut r = cov.clone();
                    for i in 0..cov.rows {
                        for j in 0..cov.cols {
                            r.set(i, j, cov.at(i, j) / (cov.at(i, i) * cov.at(j, j)).sqrt());
                        }
                    }
                    vm(r)
                }
            }
            "Cov" => {
                if m.vec {
                    let o = from_value(a.first().unwrap_or(&Value::Void), true);
                    let mut both = Mat::new(2, m.size().min(o.size()));
                    for i in 0..both.cols {
                        both.set(0, i, m.data[i]);
                        both.set(1, i, o.data[i]);
                    }
                    vm(covariance(&both))
                } else {
                    let rowvar = a.first().map(|v| v.truthy()).unwrap_or(true);
                    let src = if rowvar { m.clone() } else { m.transpose() };
                    vm(covariance(&src))
                }
            }
            "Correlate" | "Convolve" => {
                let o = from_value(a.first().unwrap_or(&Value::Void), true);
                let (x, mut y) = (m.data.clone(), o.data.clone());
                if name == "Convolve" {
                    y.reverse();
                }
                // full mode
                let n = x.len() + y.len() - 1;
                let mut out = vec![0.0; n];
                for (i, xv) in x.iter().enumerate() {
                    for (j, yv) in y.iter().enumerate() {
                        let k = if name == "Convolve" {
                            i + j
                        } else {
                            i + y.len() - 1 - j
                        };
                        out[k] += xv * yv;
                    }
                }
                vm(Mat::vector(out))
            }
            "LinearRegression" => {
                // values of the least-squares line through (index, value)
                let n = m.size() as f64;
                let xm = (n - 1.0) / 2.0;
                let ym = mean(&m.data);
                let (mut sxy, mut sxx) = (0.0, 0.0);
                for (i, y) in m.data.iter().enumerate() {
                    sxy += (i as f64 - xm) * (y - ym);
                    sxx += (i as f64 - xm).powi(2);
                }
                let b = if sxx > 0.0 { sxy / sxx } else { 0.0 };
                vm(Mat::vector(
                    (0..m.size()).map(|i| ym + b * (i as f64 - xm)).collect(),
                ))
            }
            _ => {
                return err(
                    self,
                    &format!(
                        "'{}' - unknown {} method",
                        name,
                        if m.vec { "vector" } else { "matrix" }
                    ),
                )
            }
        })
    }

    /// `m.CopyRates(symbol, period, rates_mask, from, count)`: one row per selected field
    /// (open, high, low, close, time, tick volume, real volume, spread), oldest bar first.
    /// `from` is a start position counted from the newest bar, or a date.
    fn mat_copy_rates(&mut self, mr: &MatRef, a: &[Value], host: &mut dyn Host) -> R<Value> {
        let symbol = self.sym_arg(a.first());
        let tf = self.tf_arg(a.get(1));
        let mask = ai(a, 2, 15);
        let from = ai(a, 3, 0);
        let count = ai(a, 4, 0).max(0) as usize;
        let is_vec = mr.borrow().vec;
        let Some(bars) = host.bars(&symbol, tf).map(|b| b.to_vec()) else {
            return Ok(Value::Bool(false));
        };
        let sel: Vec<crate::market::Bar> = if from > 100_000_000 {
            // from a date: `count` bars starting at that time, or up to a second date
            let start = bars.partition_point(|b| b.time < from);
            if count as i64 > 100_000_000 {
                let to = count as i64;
                bars[start..]
                    .iter()
                    .take_while(|b| b.time <= to)
                    .copied()
                    .collect()
            } else {
                bars[start..(start + count).min(bars.len())].to_vec()
            }
        } else {
            let end = bars.len().saturating_sub(from.max(0) as usize);
            bars[end.saturating_sub(count)..end].to_vec()
        };
        let fields: Vec<fn(&crate::market::Bar) -> f64> = [
            (
                1,
                (|b: &crate::market::Bar| b.open) as fn(&crate::market::Bar) -> f64,
            ),
            (2, |b| b.high),
            (4, |b| b.low),
            (8, |b| b.close),
            (16, |b| b.time as f64),
            (32, |b| b.tick_volume as f64),
            (64, |_| 0.0),
            (128, |b| b.spread as f64),
        ]
        .iter()
        .filter(|(bit, _)| mask & bit != 0)
        .map(|(_, f)| *f)
        .collect();
        if fields.is_empty() {
            return Ok(Value::Bool(false));
        }
        let vertical = mask & 0x8000 != 0; // COPY_RATES_VERTICAL: one column per field
        let n = sel.len();
        let m = if is_vec {
            Mat::vector(sel.iter().map(fields[0]).collect())
        } else if vertical {
            let mut m = Mat::new(n, fields.len());
            for (r, b) in sel.iter().enumerate() {
                for (c, f) in fields.iter().enumerate() {
                    m.set(r, c, f(b));
                }
            }
            m
        } else {
            let mut m = Mat::new(fields.len(), n);
            for (r, f) in fields.iter().enumerate() {
                for (c, b) in sel.iter().enumerate() {
                    m.set(r, c, f(b));
                }
            }
            m
        };
        *mr.borrow_mut() = m;
        let _ = Timeframe::M1;
        Ok(Value::Bool(n > 0))
    }
}

/// Covariance of the rows of `m` (each row is a variable, columns are observations).
fn covariance(m: &Mat) -> Mat {
    let n = m.cols as f64;
    let means: Vec<f64> = (0..m.rows).map(|r| mean(&m.row(r).data)).collect();
    let mut c = Mat::new(m.rows, m.rows);
    for i in 0..m.rows {
        for j in 0..m.rows {
            let s: f64 = (0..m.cols)
                .map(|k| (m.at(i, k) - means[i]) * (m.at(j, k) - means[j]))
                .sum();
            c.set(i, j, s / (n - 1.0));
        }
    }
    c
}
