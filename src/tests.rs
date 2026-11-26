//! Engine tests: every sample compiles, indicators fill their buffers and the sample
//! experts trade in the strategy tester.

use crate::market::{Market, Timeframe};
use crate::mql::parser::compile;
use crate::samples::{SAMPLES, TEMPLATE_EXPERT, TEMPLATE_INDICATOR, TEMPLATE_SCRIPT};
use crate::tester::{TestRun, TickMode};
use std::collections::HashMap;
use std::rc::Rc;

fn no_loader(_: &str) -> Option<String> {
    None
}

fn run_ea(name: &str, src: &str) -> TestRun {
    let prog =
        Rc::new(compile(name, src, &no_loader).unwrap_or_else(|e| panic!("{}: {:?}", name, e)));
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(
        prog,
        spec,
        Timeframe::H1,
        bars,
        TickMode::OhlcPoints,
        10000.0,
        100,
        &HashMap::new(),
        Box::new(|n| Err(format!("no {}", n))),
    );
    while !run.step(200) {}
    run
}

#[test]
fn samples_compile() {
    for (name, src) in SAMPLES.iter().chain(
        [
            ("t1.mq5", TEMPLATE_INDICATOR),
            ("t2.mq5", TEMPLATE_EXPERT),
            ("t3.mq5", TEMPLATE_SCRIPT),
        ]
        .iter(),
    ) {
        if let Err(e) = compile(name, src, &no_loader) {
            panic!("{} failed: {:?}", name, e);
        }
    }
}

#[test]
fn compile_errors_reported() {
    let e = compile("x.mq5", "void OnStart(){ int a = b; }", &no_loader).unwrap_err();
    assert!(e[0].msg.contains("'b' - undeclared identifier"), "{:?}", e);
    let e = compile("x.mq5", "void OnStart(){ int a = 1 }", &no_loader).unwrap_err();
    assert!(e[0].msg.contains("semicolon"), "{:?}", e);
}

#[test]
fn ma_cross_trades() {
    let src = SAMPLES
        .iter()
        .find(|(n, _)| n.contains("MA Crossover"))
        .unwrap()
        .1;
    let run = run_ea("ma.mq5", src);
    assert!(run.error.is_none(), "{:?} {:?}", run.error, run.host.log);
    let r = run.report();
    assert!(
        r.trades > 0,
        "no trades: {:?}",
        &run.host.log[..run.host.log.len().min(20)]
    );
}

#[test]
fn rsi_ea_runs() {
    let src = SAMPLES
        .iter()
        .find(|(n, _)| n.contains("RSI Reversal"))
        .unwrap()
        .1;
    let run = run_ea("rsi.mq5", src);
    assert!(run.error.is_none(), "{:?}", run.error);
}

#[test]
fn indicators_fill_buffers() {
    for (name, src) in SAMPLES
        .iter()
        .filter(|(n, _)| n.starts_with("Indicators/"))
        .chain([("t1.mq5", TEMPLATE_INDICATOR)].iter())
    {
        let prog = Rc::new(compile(name, src, &no_loader).unwrap());
        let mut m = Market::new_simulated();
        let bars = m.bars(0, Timeframe::H1).to_vec();
        let spec = m.symbols[0].spec.clone();
        let mut run = TestRun::new(
            prog.clone(),
            spec,
            Timeframe::H1,
            bars.clone(),
            TickMode::OpenPrices,
            10000.0,
            100,
            &HashMap::new(),
            Box::new(|n| Err(format!("no {}", n))),
        );
        run.host.view = bars.clone();
        let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
        inst.init(&mut run.host).unwrap();
        inst.calculate(&mut run.host)
            .unwrap_or_else(|e| panic!("{}: {}", name, inst.fmt_err(&e)));
        let plots = inst.plot_data();
        assert!(!plots.is_empty(), "{} has no plots", name);
        let data = &plots[0].1[0];
        let valid = data.iter().filter(|v| v.is_finite()).count();
        assert!(valid > bars.len() / 2, "{}: only {} values", name, valid);
        // one more tick: incremental update
        inst.calculate(&mut run.host).unwrap();
    }
}

#[test]
fn language_features() {
    let src = r#"
enum Mode { A = 1, B, C };
struct Pt { double x; double y; };
int fib(int n) { return n < 2 ? n : fib(n-1) + fib(n-2); }
void inc(int &v) { v++; }
double out[];
void OnStart() {
   int a = 7 / 2; double b = 7 / 2.0; string s = "x" + IntegerToString(a);
   Pt p; p.x = 1.5; p.y = p.x * 2;
   int arr[5] = {1,2,3,4,5}; int sum = 0;
   for(int i=0;i<ArraySize(arr);i++) { if(arr[i]==3) continue; sum += arr[i]; }
   int k = 0; inc(k); inc(k);
   ArrayResize(out, 3); ArraySetAsSeries(out, true); out[0] = 9;
   static int st = 0; st++;
   switch(B) { case 1: sum = -1; break; case 2: sum += 100; break; default: sum = -2; }
   Print(a, " ", b, " ", s, " ", p.y, " ", sum, " ", fib(10), " ", k, " ", out[0], " ", StringFormat("%05.1f|%-3s|%d", 3.14159, "ab", 42));
}
"#;
    let prog = Rc::new(compile("lang.mq5", src, &no_loader).unwrap_or_else(|e| panic!("{:?}", e)));
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(
        prog.clone(),
        spec,
        Timeframe::H1,
        bars,
        TickMode::OpenPrices,
        10000.0,
        100,
        &HashMap::new(),
        Box::new(|n| Err(format!("no {}", n))),
    );
    let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
    inst.init(&mut run.host).unwrap();
    inst.call_event("OnStart", vec![], &mut run.host)
        .unwrap_or_else(|e| panic!("{}", inst.fmt_err(&e)));
    let last = run.host.log.last().unwrap().clone();
    assert!(
        last.ends_with("3 3.5 x3 3 112 55 2 9 003.1|ab |42"),
        "{}",
        last
    );
}

#[test]
fn runtime_errors() {
    let src = "void OnStart(){ int z = 0; int a[3]; int x = 5 / z; }";
    let prog = Rc::new(compile("err.mq5", src, &no_loader).unwrap());
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(
        prog.clone(),
        spec,
        Timeframe::H1,
        bars,
        TickMode::OpenPrices,
        10000.0,
        100,
        &HashMap::new(),
        Box::new(|n| Err(format!("no {}", n))),
    );
    let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
    inst.init(&mut run.host).unwrap();
    let e = inst
        .call_event("OnStart", vec![], &mut run.host)
        .unwrap_err();
    assert_eq!(e.msg, "zero divide");
}

/// Compile and run a script's OnStart, returning what it printed.
fn run_script(src: &str) -> Vec<String> {
    let prog =
        Rc::new(compile("script.mq5", src, &no_loader).unwrap_or_else(|e| panic!("{:?}", e)));
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(
        prog.clone(),
        spec,
        Timeframe::H1,
        bars,
        TickMode::OpenPrices,
        10000.0,
        100,
        &HashMap::new(),
        Box::new(|n| Err(format!("no {}", n))),
    );
    let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
    inst.init(&mut run.host)
        .unwrap_or_else(|e| panic!("{}", inst.fmt_err(&e)));
    inst.call_event("OnStart", vec![], &mut run.host)
        .unwrap_or_else(|e| panic!("{}", inst.fmt_err(&e)));
    // log lines look like "time\tprogram\tmessage" or similar: keep only the message text
    run.host
        .log
        .iter()
        .map(|l| l.rsplit('\t').next().unwrap_or(l).to_string())
        .collect()
}

fn last_line(src: &str) -> String {
    run_script(src).last().cloned().unwrap_or_default()
}

#[test]
fn preprocessor() {
    let src = r#"
#define SQR(x) ((x)*(x))
#define MAX(a,b) ((a)>(b)?(a):(b))
#define STR(x) #x
#define CAT(a,b) a##b
#define TWICE(x) (SQR(x)*2)
#define FLAG
#ifdef FLAG
int mode = 1;
#else
int mode = 2;
#endif
#ifndef FLAG
int other = 5;
#else
int other = 6;
#endif
#ifdef __MQL5__
int mql = 5;
#endif
#ifdef __MQL4__
int mql = 4;
#endif
void OnStart() {
   int CAT(my,Var) = 3;
   Print(SQR(1+2), " ", MAX(4, 9), " ", STR(hello), " ", myVar, " ", TWICE(3), " ", mode, " ", other, " ", mql, " ", __FUNCTION__);
}
"#;
    let out = last_line(src);
    assert!(out.ends_with("9 9 hello 3 18 1 6 5 OnStart"), "{}", out);
}

#[test]
fn classes() {
    let src = r#"
class CShape {
protected:
   string m_name;
   static int s_count;
public:
   CShape(string name) : m_name(name) { s_count++; }
   virtual ~CShape() { Print("bye ", m_name); }
   virtual double Area() { return 0; }
   string Name() const { return m_name; }
   string Describe() { return m_name + "=" + DoubleToString(Area(), 1); }
   static int Count() { return s_count; }
};
int CShape::s_count = 0;

class CRect : public CShape {
private:
   double m_w, m_h;
public:
   CRect(double w, double h) : CShape("rect"), m_w(w), m_h(h) {}
   virtual double Area() override { return m_w * m_h; }
};

class CSquare : public CRect {
public:
   CSquare(double s) : CRect(s, s) { m_name = "square"; }
};

class CCounter {
   int m_n;
public:
   CCounter() { m_n = 10; }
   void Add(int k) { m_n += k; }
   void Add() { Add(1); }
   int Get() { return this.m_n; }
   bool operator==(const CCounter &o) { return m_n == o.Get(); }
};

class CHolder {
public:
   CCounter c;
   CShape *items[];
   void Push(CShape *s) { int n = ArraySize(items); ArrayResize(items, n + 1); items[n] = s; }
};

template<typename T>
T Biggest(T a, T b) { return a > b ? a : b; }

template<typename T>
class CBox {
   T m_v;
public:
   void Set(T v) { m_v = v; }
   T Get() { return m_v; }
};

void OnStart() {
   CRect r(2, 3);
   CShape *s = new CSquare(4);
   CHolder h;
   h.c.Add(); h.c.Add(5);
   h.Push(GetPointer(r)); h.Push(s);
   double total = 0;
   for(int i = 0; i < ArraySize(h.items); i++) total += h.items[i].Area();
   CCounter a, b;
   CBox<string> box; box.Set("hi");
   CShape *none = NULL;
   Print(r.Describe(), " ", s.Describe(), " ", total, " ", h.c.Get(), " ", CShape::Count(), " ", a == b, " ",
         Biggest(3, 8), " ", Biggest(2.5, 1.5), " ", box.Get(), " ", none == NULL, " ", s != NULL, " ", CheckPointer(s) != POINTER_INVALID);
   delete s;
}
"#;
    let lines = run_script(src);
    let joined = lines.join("\n");
    assert!(
        joined.contains("rect=6.0 square=16.0 22 16 2 true 8 2.5 hi true true true"),
        "{}",
        joined
    );
    assert!(lines.last().unwrap().ends_with("bye square"), "{}", joined);
}

/// Compatibility report over a folder of real programs (not run by default):
/// `CORPUS=/path/to/MQL5 cargo test corpus -- --ignored --nocapture`
/// Every .mq5 under Indicators/, Experts/ and Scripts/ is compiled; indicators run
/// OnCalculate, experts run a short strategy test. Includes come from <CORPUS>/Include.
#[test]
#[ignore]
fn corpus() {
    use std::path::{Path, PathBuf};
    let root = PathBuf::from(std::env::var("CORPUS").expect("set CORPUS to an MQL5 folder"));
    let only = std::env::var("CORPUS_FILTER").unwrap_or_default();
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "mq5") {
                    out.push(p);
                }
            }
        }
    }
    let mut files = vec![];
    for sub in ["Indicators", "Experts", "Scripts"] {
        walk(&root.join(sub), &mut files);
    }
    files.sort();
    files.retain(|f| f.to_string_lossy().contains(&only));
    let inc_root = root.join("Include");
    let ind_root = root.join("Indicators");
    let (mut ok_c, mut ok_r) = (0, 0);
    let mut errs: HashMap<String, Vec<String>> = HashMap::new();
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        let src = match std::fs::read(f) {
            Ok(b) => crate::storage::decode_source(&b),
            Err(_) => continue,
        };
        let base = f.parent().unwrap().to_path_buf();
        let inc = inc_root.clone();
        let loader = move |p: &str| -> Option<String> {
            [base.join(p), inc.join(p)]
                .iter()
                .find_map(|c| std::fs::read(c).ok())
                .map(|b| crate::storage::decode_source(&b))
        };
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let prog = match compile(&name, &src, &loader) {
            Ok(p) => Rc::new(p),
            Err(e) => {
                let first = e
                    .first()
                    .map(|e| format!("{} (line {})", e.msg, e.line))
                    .unwrap_or_default();
                let key = first.split(" (line").next().unwrap_or("").to_string();
                let key = key.split(" - ").last().unwrap_or(&key).to_string();
                println!("COMPILE FAIL {}: {}", rel, first);
                for x in &e {
                    println!("ERR {}", x.msg);
                }
                errs.entry(format!("compile: {}", key))
                    .or_default()
                    .push(rel);
                continue;
            }
        };
        ok_c += 1;
        let ind_root2 = ind_root.clone();
        let inc2 = inc_root.clone();
        let prog_loader: Box<dyn Fn(&str) -> Result<Rc<crate::mql::Program>, String>> =
            Box::new(move |n: &str| {
                let rel = n.replace('\\', "/");
                let rel = if rel.ends_with(".mq5") {
                    rel
                } else {
                    format!("{}.mq5", rel)
                };
                let p = ind_root2.join(&rel);
                let b = std::fs::read(&p).map_err(|e| format!("{}: {}", rel, e))?;
                let src = crate::storage::decode_source(&b);
                let base = p.parent().unwrap().to_path_buf();
                let inc = inc2.clone();
                let l = move |q: &str| -> Option<String> {
                    [base.join(q), inc.join(q)]
                        .iter()
                        .find_map(|c| std::fs::read(c).ok())
                        .map(|b| crate::storage::decode_source(&b))
                };
                compile(&rel, &src, &l)
                    .map(Rc::new)
                    .map_err(|e| format!("{:?}", e.first()))
            });
        let result: Result<(), String> = (|| match prog.kind {
            crate::mql::ProgramKind::Indicator => {
                let mut run = TestRun::new(
                    prog.clone(),
                    spec.clone(),
                    Timeframe::H1,
                    bars.clone(),
                    TickMode::OpenPrices,
                    10000.0,
                    100,
                    &HashMap::new(),
                    prog_loader,
                );
                run.host.view = bars.clone();
                let mut inst = crate::mql::Instance::new(
                    prog.clone(),
                    "EURUSD",
                    Timeframe::H1,
                    &HashMap::new(),
                );
                let code = inst.init(&mut run.host).map_err(|e| inst.fmt_err(&e))?;
                if code != 0 {
                    return Err(format!("OnInit returned {}", code));
                }
                inst.calculate(&mut run.host)
                    .map_err(|e| inst.fmt_err(&e))?;
                inst.calculate(&mut run.host)
                    .map_err(|e| inst.fmt_err(&e))?;
                Ok(())
            }
            crate::mql::ProgramKind::Expert => {
                let short: Vec<crate::market::Bar> = bars[..600.min(bars.len())].to_vec();
                let mut run = TestRun::new(
                    prog.clone(),
                    spec.clone(),
                    Timeframe::H1,
                    short,
                    TickMode::OpenPrices,
                    10000.0,
                    100,
                    &HashMap::new(),
                    prog_loader,
                );
                let mut guard = 0;
                while !run.step(100) && guard < 1000 {
                    guard += 1;
                }
                match &run.error {
                    Some(e) => Err(e.clone()),
                    None => Ok(()),
                }
            }
            crate::mql::ProgramKind::Script if std::env::var("CORPUS_SCRIPTS").is_ok() => {
                let mut run = TestRun::new(
                    prog.clone(),
                    spec.clone(),
                    Timeframe::H1,
                    bars.clone(),
                    TickMode::OpenPrices,
                    10000.0,
                    100,
                    &HashMap::new(),
                    prog_loader,
                );
                run.host.view = bars.clone();
                let r = run
                    .inst
                    .call_event("OnStart", vec![], &mut run.host)
                    .map(|_| ())
                    .map_err(|e| run.inst.fmt_err(&e));
                for l in &run.host.log {
                    println!("LOG {}", l);
                }
                r
            }
            _ => Ok(()),
        })();
        match result {
            Ok(()) => ok_r += 1,
            Err(e) => {
                println!("RUN FAIL {}: {}", rel, e);
                let key = e.split(" in '").next().unwrap_or(&e).to_string();
                errs.entry(format!("run: {}", key)).or_default().push(rel);
            }
        }
    }
    let mut v: Vec<_> = errs.into_iter().collect();
    v.sort_by_key(|(_, f)| std::cmp::Reverse(f.len()));
    println!(
        "\n==== {} files: {} compile, {} compile+run OK ====",
        files.len(),
        ok_c,
        ok_r
    );
    for (k, f) in v.iter().take(60) {
        println!("{:4}  {}", f.len(), k);
    }
}

/// Compare the built-in indicators with the platform's own example indicators
/// (Indicators/Examples/*.mq5) run in the engine, value by value:
/// `CORPUS=/path/to/MQL5 cargo test reference_indicators -- --ignored --nocapture`
#[test]
#[ignore]
fn reference_indicators() {
    use crate::indicators::{IndKind, IndParams};
    use crate::indicators_ext::{ExtKind, ExtParams};
    let script_buffers =
        |k: IndKind, p: &IndParams, b: &[crate::market::Bar]| crate::mt5ind::builtin(k, p, b);
    let ext = |k: ExtKind, p: &ExtParams, b: &[crate::market::Bar]| {
        crate::mt5ind::ext(k, p, b, 0.00001)
            .unwrap_or_else(|| crate::indicators_ext::compute(k, p, b))
    };
    let root = std::path::PathBuf::from(std::env::var("CORPUS").expect("set CORPUS"));
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let d = |k: IndKind| IndParams::defaults(k);
    let e = |k: ExtKind, i: &[i64]| {
        ext(
            k,
            &ExtParams {
                i: i.to_vec(),
                f: vec![],
            },
            &bars,
        )
    };
    type Case = (&'static str, Vec<Vec<f64>>, Vec<(usize, usize)>);
    let cases: Vec<Case> = vec![
        (
            "RSI",
            script_buffers(
                IndKind::Rsi,
                &IndParams {
                    p1: 14,
                    ..d(IndKind::Rsi)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "MACD",
            script_buffers(
                IndKind::Macd,
                &IndParams {
                    p1: 12,
                    p2: 26,
                    p3: 9,
                    ..d(IndKind::Macd)
                },
                &bars,
            ),
            vec![(0, 0), (1, 1)],
        ),
        (
            "ATR",
            script_buffers(
                IndKind::Atr,
                &IndParams {
                    p1: 14,
                    ..d(IndKind::Atr)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "BB",
            script_buffers(
                IndKind::Bands,
                &IndParams {
                    p1: 20,
                    f1: 2.0,
                    ..d(IndKind::Bands)
                },
                &bars,
            ),
            vec![(0, 0), (1, 1), (2, 2)],
        ),
        (
            "CCI",
            script_buffers(
                IndKind::Cci,
                &IndParams {
                    p1: 14,
                    price: 1,
                    ..d(IndKind::Cci)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "Momentum",
            script_buffers(
                IndKind::Momentum,
                &IndParams {
                    p1: 14,
                    ..d(IndKind::Momentum)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "Stochastic",
            script_buffers(
                IndKind::Stochastic,
                &IndParams {
                    p1: 5,
                    p2: 3,
                    p3: 3,
                    ..d(IndKind::Stochastic)
                },
                &bars,
            ),
            vec![(0, 0), (1, 1)],
        ),
        (
            "ADX",
            script_buffers(
                IndKind::Adx,
                &IndParams {
                    p1: 14,
                    ..d(IndKind::Adx)
                },
                &bars,
            ),
            vec![(0, 0), (1, 1), (2, 2)],
        ),
        (
            "WPR",
            script_buffers(
                IndKind::Wpr,
                &IndParams {
                    p1: 14,
                    ..d(IndKind::Wpr)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "Custom Moving Average",
            script_buffers(
                IndKind::MovingAverage,
                &IndParams {
                    p1: 13,
                    method: 2,
                    ..d(IndKind::MovingAverage)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "Envelopes",
            script_buffers(
                IndKind::Envelopes,
                &IndParams {
                    p1: 14,
                    f1: 0.1,
                    ..d(IndKind::Envelopes)
                },
                &bars,
            ),
            vec![(0, 0), (1, 1)],
        ),
        (
            "Ichimoku",
            script_buffers(
                IndKind::Ichimoku,
                &IndParams {
                    p1: 9,
                    p2: 26,
                    p3: 52,
                    ..d(IndKind::Ichimoku)
                },
                &bars,
            ),
            vec![(0, 0), (1, 1)],
        ),
        (
            "ParabolicSAR",
            script_buffers(
                IndKind::Sar,
                &IndParams {
                    f1: 0.02,
                    f2: 0.2,
                    ..d(IndKind::Sar)
                },
                &bars,
            ),
            vec![(0, 0)],
        ),
        (
            "Alligator",
            script_buffers(IndKind::Alligator, &d(IndKind::Alligator), &bars),
            vec![(0, 0), (1, 1), (2, 2)],
        ),
        (
            "Fractals",
            script_buffers(IndKind::Fractals, &d(IndKind::Fractals), &bars),
            vec![(0, 0), (1, 1)],
        ),
        ("DEMA", e(ExtKind::Dema, &[14, 0, 1]), vec![(0, 0)]),
        ("TEMA", e(ExtKind::Tema, &[14, 0, 1]), vec![(0, 0)]),
        ("TRIX", e(ExtKind::Trix, &[14, 1]), vec![(0, 0)]),
        ("OsMA", e(ExtKind::Osma, &[12, 26, 9, 1]), vec![(0, 0)]),
        (
            "Awesome_Oscillator",
            e(ExtKind::Ao, &[]),
            vec![(0, 0), (1, 1)],
        ),
        ("Accelerator", e(ExtKind::Ac, &[]), vec![(0, 0), (1, 1)]),
        ("Bears", e(ExtKind::Bears, &[13]), vec![(0, 0)]),
        ("Bulls", e(ExtKind::Bulls, &[13]), vec![(0, 0)]),
        ("Force_Index", e(ExtKind::Force, &[13, 0, 0]), vec![(0, 0)]),
        ("MFI", e(ExtKind::Mfi, &[14, 0]), vec![(0, 0)]),
        ("OBV", e(ExtKind::Obv, &[0]), vec![(0, 0)]),
        ("RVI", e(ExtKind::Rvi, &[10]), vec![(0, 0), (1, 1)]),
        ("StdDev", e(ExtKind::StdDev, &[20, 0, 0, 1]), vec![(0, 0)]),
        ("DeMarker", e(ExtKind::DeMarker, &[14]), vec![(0, 0)]),
        ("CHO", e(ExtKind::Chaikin, &[3, 10, 1, 0]), vec![(0, 0)]),
        ("AD", e(ExtKind::Ad, &[0]), vec![(0, 0)]),
        (
            "MarketFacilitationIndex",
            e(ExtKind::Bwmfi, &[0]),
            vec![(0, 0)],
        ),
        (
            "Gator",
            e(ExtKind::Gator, &[13, 8, 8, 5, 5, 3, 2, 5]),
            vec![(0, 0), (2, 2)],
        ),
        ("AMA", e(ExtKind::Ama, &[10, 2, 30, 0, 1]), vec![(0, 0)]),
        ("FrAMA", e(ExtKind::Frama, &[14, 0, 1]), vec![(0, 0)]),
        ("VIDYA", e(ExtKind::Vidya, &[9, 12, 0, 1]), vec![(0, 0)]),
        (
            "ADXW",
            e(ExtKind::AdxWilder, &[14]),
            vec![(0, 0), (1, 1), (2, 2)],
        ),
    ];
    let inc = root.join("Include");
    let mut exact = 0;
    for (name, native, pairs) in cases {
        let path = root
            .join("Indicators/Examples")
            .join(format!("{}.mq5", name));
        let src = match std::fs::read(&path) {
            Ok(b) => crate::storage::decode_source(&b),
            Err(_) => {
                println!("{:24} (no file)", name);
                continue;
            }
        };
        let inc2 = inc.clone();
        let loader = move |p: &str| -> Option<String> {
            std::fs::read(inc2.join(p))
                .ok()
                .map(|b| crate::storage::decode_source(&b))
        };
        let prog = match compile(name, &src, &loader) {
            Ok(p) => Rc::new(p),
            Err(e) => {
                println!("{:24} compile error {:?}", name, e.first());
                continue;
            }
        };
        let mut run = TestRun::new(
            prog.clone(),
            spec.clone(),
            Timeframe::H1,
            bars.clone(),
            TickMode::OpenPrices,
            10000.0,
            100,
            &HashMap::new(),
            Box::new(|n| Err(format!("no {}", n))),
        );
        run.host.view = bars.clone();
        let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
        if let Err(er) = inst
            .init(&mut run.host)
            .and_then(|_| inst.calculate(&mut run.host))
        {
            println!("{:24} run error {}", name, inst.fmt_err(&er));
            continue;
        }
        let mut line = format!("{:24}", name);
        let mut all_ok = true;
        for (rb, nb) in pairs {
            let r: Vec<f64> = match inst.buffers.get(rb).and_then(|b| b.as_ref()) {
                Some((arr, _)) => arr.borrow().data.iter().map(|v| v.as_f64()).collect(),
                None => vec![],
            };
            let nv = native.get(nb).cloned().unwrap_or_default();
            let ok = |x: f64| x.is_finite() && x != f64::MAX && x.abs() < 1e300;
            let (mut cnt, mut maxd, mut maxd_tail, mut only_r, mut only_n) =
                (0, 0.0f64, 0.0f64, 0, 0);
            let n = r.len().min(nv.len());
            for i in 0..n {
                match (ok(r[i]), ok(nv[i])) {
                    (true, true) => {
                        cnt += 1;
                        let dd = (r[i] - nv[i]).abs() / r[i].abs().max(1e-9).max(1.0);
                        maxd = maxd.max(dd);
                        if i + 500 >= n {
                            maxd_tail = maxd_tail.max(dd);
                        }
                    }
                    (true, false) => only_r += 1,
                    (false, true) => only_n += 1,
                    _ => {}
                }
            }
            let good = maxd < 1e-9 && only_r == 0 && only_n == 0 && cnt > 0;
            all_ok &= good;
            line += &format!(
                " | b{}: n={} max={:.2e} tail={:.2e} onlyRef={} onlyNative={}",
                rb, cnt, maxd, maxd_tail, only_r, only_n
            );
        }
        if all_ok {
            exact += 1;
        }
        println!("{} {}", if all_ok { "OK  " } else { "DIFF" }, line);
    }
    println!("exact: {}", exact);
}

#[test]
fn history_csv_formats() {
    use crate::history::{aggregate, bar_open_time, detect_timeframe, name_hints, parse_csv};
    // MetaTrader 5 export
    let mt5 = "<DATE>\t<TIME>\t<OPEN>\t<HIGH>\t<LOW>\t<CLOSE>\t<TICKVOL>\t<VOL>\t<SPREAD>\n2024.01.02\t00:00:00\t1.10380\t1.10410\t1.10370\t1.10400\t120\t0\t5\n2024.01.02\t00:01:00\t1.10400\t1.10450\t1.10390\t1.10440\t98\t0\t6\n2024.01.02\t00:02:00\t1.10440\t1.10460\t1.10420\t1.10430\t77\t0\t5\n";
    let b = parse_csv(mt5).unwrap();
    assert_eq!(b.len(), 3);
    assert_eq!(b[0].time, 1704153600);
    assert_eq!(
        (b[1].open, b[1].high, b[1].tick_volume, b[1].spread),
        (1.104, 1.1045, 98, 6)
    );
    assert_eq!(detect_timeframe(&b), Some(Timeframe::M1));
    // MetaTrader 4 / generic without header
    let mt4 = "2024.01.02,10:00,2063.10,2065.00,2062.00,2064.50,1500\n2024.01.02,11:00,2064.50,2066.00,2060.00,2061.20,1300\n2024.01.02,12:00,2061.20,2062.00,2058.00,2059.00,900\n";
    let b = parse_csv(mt4).unwrap();
    assert_eq!(b.len(), 3);
    assert_eq!(b[0].time, 1704153600 + 36000);
    assert_eq!(detect_timeframe(&b), Some(Timeframe::H1));
    // header with one date-time column, unix milliseconds
    let ms = "timestamp,open,high,low,close,volume\n1704153600000,1,2,0.5,1.5,10\n1704153900000,1.5,2,1,1.2,11\n1704154200000,1.2,1.3,1,1.1,9\n";
    let b = parse_csv(ms).unwrap();
    assert_eq!(b[1].time, 1704153900);
    assert_eq!(detect_timeframe(&b), Some(Timeframe::M5));
    // Dukascopy-style day-first dates with milliseconds
    let dk = "Gmt time;Open;High;Low;Close;Volume\n02.01.2024 00:00:00.000;1.1;1.2;1.0;1.15;5\n";
    assert_eq!(parse_csv(dk).unwrap()[0].time, 1704153600);
    // file names
    let (s, t) = name_hints(std::path::Path::new(
        "/x/EURUSD_M1_202401020000_202411221759.csv",
    ));
    assert_eq!((s.as_deref(), t), (Some("EURUSD"), Some(Timeframe::M1)));
    // weeks start on Sunday, months on the 1st
    assert_eq!(bar_open_time(Timeframe::W1, 1704153600), 1703980800); // Tue 2024-01-02 -> Sun 2023-12-31
    assert_eq!(bar_open_time(Timeframe::MN1, 1706745599), 1704067200); // 2024-01-31 23:59:59 -> 2024-01-01
    let h1 = parse_csv(mt4).unwrap();
    let d1 = aggregate(&h1, Timeframe::D1);
    assert_eq!(d1.len(), 1);
    assert_eq!(
        (
            d1[0].open,
            d1[0].high,
            d1[0].low,
            d1[0].close,
            d1[0].tick_volume
        ),
        (2063.10, 2066.0, 2058.0, 2059.0, 3700)
    );
}

/// The export script for MetaTrader 5 compiles and runs here too, and what it writes
/// imports back as the same bars and symbol properties.
#[test]
fn export_script_round_trip() {
    let dir = std::env::temp_dir().join(format!("rt_export_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("RUST_TERMINAL_DATA", &dir);
    let prog = Rc::new(
        compile(
            "RustTerminal_Export.mq5",
            crate::history::EXPORT_SCRIPT,
            &no_loader,
        )
        .unwrap_or_else(|e| panic!("{:?}", e)),
    );
    let mut m = Market::new_simulated();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut inputs = HashMap::new();
    inputs.insert("InpTimeframe".to_string(), "PERIOD_H1".to_string());
    inputs.insert("InpDays".to_string(), "30".to_string());
    let mut run = TestRun::new(
        prog.clone(),
        spec,
        Timeframe::H1,
        bars.clone(),
        TickMode::OpenPrices,
        10000.0,
        100,
        &inputs,
        Box::new(|n| Err(format!("no {}", n))),
    );
    run.host.view = bars.clone();
    run.inst
        .call_event("OnStart", vec![], &mut run.host)
        .unwrap_or_else(|e| panic!("{}", run.inst.fmt_err(&e)));
    let out = dir.join("Common/Files/RustTerminal");
    let csv = out.join("EURUSD_H1.csv");
    assert!(csv.exists(), "log: {:?}", run.host.log);
    let imported = crate::history::import_file(&csv, None, None, crate::tz::server()).unwrap();
    assert_eq!(
        (imported.symbol.as_str(), imported.tf),
        ("EURUSD", Timeframe::H1)
    );
    let back = crate::history::load("EURUSD", Timeframe::H1).unwrap();
    // every exported bar equals the original bar with the same time
    assert!(back.len() > 50);
    for got in &back {
        let orig = bars
            .iter()
            .find(|b| b.time == got.time)
            .expect("exported bar exists");
        assert_eq!(
            (got.open, got.high, got.low, got.close, got.tick_volume),
            (orig.open, orig.high, orig.low, orig.close, orig.tick_volume)
        );
    }
    let syms = crate::history::parse_symbols_csv(
        &std::fs::read_to_string(out.join("symbols.csv")).unwrap(),
    );
    assert_eq!(
        syms.first().map(|s| (s.name.as_str(), s.digits)),
        Some(("EURUSD", 5))
    );
    std::env::remove_var("RUST_TERMINAL_DATA");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A test driven by one-minute bars: the expert reads M5 while being tested on H1,
/// and receives four ticks per minute.
#[test]
fn tester_with_minutes() {
    let src = r#"
int h5, ticks = 0;
int OnInit() { h5 = iMA(_Symbol, PERIOD_M5, 12, 0, MODE_EMA, PRICE_CLOSE); return h5 == INVALID_HANDLE ? 1 : 0; }
void OnTick() {
   ticks++;
   double m5[]; ArraySetAsSeries(m5, true);
   if(CopyBuffer(h5, 0, 0, 2, m5) < 2) { Print("no M5 data"); ExpertRemove(); return; }
   MqlRates r[]; if(CopyRates(_Symbol, PERIOD_M1, 0, 1, r) != 1) { Print("no M1 data"); ExpertRemove(); return; }
   if(PositionsTotal() == 0 && ticks % 500 == 0) {
      MqlTradeRequest q = {}; MqlTradeResult res = {};
      q.action = TRADE_ACTION_DEAL; q.symbol = _Symbol; q.volume = 0.1;
      q.type = m5[0] > m5[1] ? ORDER_TYPE_BUY : ORDER_TYPE_SELL;
      q.price = q.type == ORDER_TYPE_BUY ? SymbolInfoDouble(_Symbol, SYMBOL_ASK) : SymbolInfoDouble(_Symbol, SYMBOL_BID);
      OrderSend(q, res);
   }
}
void OnDeinit(const int reason) { Print("ticks=", ticks); }
"#;
    let prog = Rc::new(compile("m5.mq5", src, &no_loader).unwrap_or_else(|e| panic!("{:?}", e)));
    let m = Market::new_simulated();
    let spec = m.symbols[0].spec.clone();
    // 6000 synthetic minutes
    let mut minutes = vec![];
    let mut p = 1.1;
    let start = 1_704_153_600;
    for i in 0..6000i64 {
        let o = p;
        p += ((i * 7919 % 13) as f64 - 6.0) * 0.00003;
        minutes.push(crate::market::Bar {
            time: start + i * 60,
            open: o,
            high: o.max(p) + 0.00005,
            low: o.min(p) - 0.00005,
            close: p,
            tick_volume: 10,
            spread: 10,
        });
    }
    let mut run = TestRun::with_minutes(
        prog,
        spec,
        Timeframe::H1,
        minutes,
        TickMode::OhlcPoints,
        10000.0,
        100,
        &HashMap::new(),
        Box::new(|n| Err(format!("no {}", n))),
    );
    while !run.step(50) {}
    assert!(run.error.is_none(), "{:?}", run.error);
    let log = run.host.log.join("\n");
    assert!(
        !log.contains("no M5 data") && !log.contains("no M1 data"),
        "{}",
        log
    );
    // 100 H1 bars of warm-up are skipped by the tester (25 here); the rest tick 4x per minute
    assert!(run.ticks > 4 * 4000, "ticks {}", run.ticks);
    assert!(run.host.account.deals.len() >= 2, "{}", log);
}

/// Needs the network: `cargo test live_feed -- --ignored --nocapture`
#[test]
#[ignore]
fn live_feed() {
    let dir = std::env::temp_dir().join("rt_live_feed_test");
    std::env::set_var("RUST_TERMINAL_DATA", &dir);
    let mut m = Market::new_simulated();
    for s in ["AAPL", "NQ", "BTCUSDT.P"] {
        m.add_symbol(s).unwrap();
    }
    let n = m.start_live();
    assert!(n >= 4, "{} symbols with sources", n);
    let want = ["BTCUSD", "EURUSD", "XAUUSD", "AAPL", "NQ"];
    let idx: Vec<usize> = want.iter().map(|s| m.symbol_index(s).unwrap()).collect();
    let t0 = std::time::Instant::now();
    let mut got = vec![false; want.len()];
    while t0.elapsed() < std::time::Duration::from_secs(120) && !got.iter().all(|g| *g) {
        m.update();
        for (k, i) in idx.iter().enumerate() {
            if m.symbols[*i].provider.is_some() && m.symbols[*i].ticks > 0 {
                got[k] = true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    for l in &m.feed_log {
        println!("feed: {}", l);
    }
    for i in idx.iter().copied().chain(m.symbol_index("BTCUSDT.P")) {
        let s = &m.symbols[i];
        let h1 = m
            .bars_if_loaded(i, Timeframe::H1)
            .map(|b| b.len())
            .unwrap_or(0);
        println!(
            "{:10} {:?} via {:?}: bid {} ask {} digits {} H1 bars {}",
            s.spec.name, s.source, s.provider, s.bid, s.ask, s.spec.digits, h1
        );
    }
    std::env::remove_var("RUST_TERMINAL_DATA");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(got[0], "BTCUSD got no live price: {}", m.feed_status);
}

#[test]
fn matrix_vector() {
    let out = run_script(
        r#"
double Sq(vector &v) { return v.Dot(v); }
void OnStart() {
   matrix a = {{1, 2}, {3, 4}};
   matrix b = matrix::Eye(2, 2);
   matrix c = a.MatMul(b) + 1.0;
   Print(c[1][0]);
   matrix i = a.Inv();
   PrintFormat("%.2f %.2f", a.Det(), i[0][0]);
   vector v = {3, 4};
   Print(v.Norm(VECTOR_NORM_P), " ", Sq(v), " ", v.Size());
   vector w(3);
   w[2] = 7;
   w.Resize(4);
   Print(w.Sum(), " ", w.Size());
   matrix z(2, 3);
   z.Fill(2);
   z[1][2] = 5;
   matrix t = z.Transpose();
   Print(t.Rows(), "x", t.Cols(), " ", t[2][1]);
   matrix copy = a;
   copy[0][0] = 100;
   Print(a[0][0]);
   vector s = a.Sum(0);
   Print(s);
   vector e = MathExp(vector::Zeros(2));
   Print(e[1]);
   matrix r;
   bool ok = r.CopyRates(_Symbol, PERIOD_H1, COPY_RATES_OHLC, 0, 10);
   Print(ok, " ", r.Rows(), " ", r.Cols());
   vector cl;
   cl.CopyRates(_Symbol, PERIOD_H1, COPY_RATES_CLOSE, 0, 5);
   Print(cl.Size(), " ", cl[4] == iClose(_Symbol, PERIOD_H1, 0));
   vector x = a.Solve(vector::Ones(2) * 3);
   PrintFormat("%.1f %.1f", x[0], x[1]);
}
"#,
    );
    let want = [
        "4",
        "-2.00 -2.00",
        "5 25 2",
        "7 4",
        "3x2 5",
        "1",
        "[4,6]",
        "1",
        "true 4 10",
        "5 true",
        "-3.0 3.0",
    ];
    let got: Vec<&str> = out
        .iter()
        .map(|s| s.split("   ").last().unwrap_or(s).trim())
        .collect();
    assert!(got.ends_with(&want), "{:#?}", got);
}

#[test]
fn copybuffer_shift() {
    let out = run_script(
        r#"
void OnStart() {
   int h0 = iMA(_Symbol, PERIOD_H1, 10, 0, MODE_SMA, PRICE_CLOSE);
   int h3 = iMA(_Symbol, PERIOD_H1, 10, 3, MODE_SMA, PRICE_CLOSE);
   double a[], b[];
   CopyBuffer(h0, 0, 3, 1, a);
   CopyBuffer(h3, 0, 0, 1, b);
   Print(a[0] == b[0]);
   int al = iAlligator(_Symbol, PERIOD_H1, 13, 8, 8, 5, 5, 3, MODE_SMMA, PRICE_MEDIAN);
   int an = iAlligator(_Symbol, PERIOD_H1, 13, 0, 8, 0, 5, 0, MODE_SMMA, PRICE_MEDIAN);
   double j[], k[], t[], u[];
   CopyBuffer(al, 0, 0, 1, j);
   CopyBuffer(an, 0, 8, 1, k);
   CopyBuffer(al, 1, 0, 1, t);
   CopyBuffer(an, 1, 5, 1, u);
   Print(j[0] == k[0], " ", t[0] == u[0]);
}
"#,
    );
    let got: Vec<&str> = out
        .iter()
        .map(|s| s.split("   ").last().unwrap_or(s).trim())
        .collect();
    assert!(got.ends_with(&["true", "true true"]), "{:#?}", got);
}

#[test]
fn time_zones() {
    use crate::tz::{us_dst, Zone};
    let d = |y: i64, m: i64, day: i64, h: i64| {
        crate::mql::lexer::days_from_civil(y, m, day) * 86400 + h * 3600
    };
    // 2026: DST from Sunday 8 March 07:00 UTC to Sunday 1 November 06:00 UTC
    assert!(!us_dst(d(2026, 3, 8, 6)));
    assert!(us_dst(d(2026, 3, 8, 7)));
    assert!(us_dst(d(2026, 11, 1, 5)));
    assert!(!us_dst(d(2026, 11, 1, 6)));
    assert_eq!(Zone::Broker.offset_at(d(2026, 7, 1, 0)), 3 * 3600);
    assert_eq!(Zone::Broker.offset_at(d(2026, 12, 1, 0)), 2 * 3600);
    assert_eq!(Zone::parse("+07:00"), Some(Zone::Fixed(420)));
    assert_eq!(Zone::parse("GMT-5"), Some(Zone::Fixed(-300)));
    assert_eq!(Zone::parse("broker"), Some(Zone::Broker));
    for z in [Zone::Broker, Zone::Fixed(420), Zone::Utc] {
        let t = d(2026, 6, 15, 12);
        assert_eq!(z.to_utc(z.from_utc(t)), t);
        assert_eq!(Zone::parse(&z.key()), Some(z));
    }
}
