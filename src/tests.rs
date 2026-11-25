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
    let prog = Rc::new(compile(name, src, &no_loader).unwrap_or_else(|e| panic!("{}: {:?}", name, e)));
    let mut m = Market::new();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(prog, spec, Timeframe::H1, bars, TickMode::OhlcPoints, 10000.0, 100, &HashMap::new(), Box::new(|n| Err(format!("no {}", n))));
    while !run.step(200) {}
    run
}

#[test]
fn samples_compile() {
    for (name, src) in SAMPLES.iter().chain([("t1.mq5", TEMPLATE_INDICATOR), ("t2.mq5", TEMPLATE_EXPERT), ("t3.mq5", TEMPLATE_SCRIPT)].iter()) {
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
    let src = SAMPLES.iter().find(|(n, _)| n.contains("MA Crossover")).unwrap().1;
    let run = run_ea("ma.mq5", src);
    assert!(run.error.is_none(), "{:?} {:?}", run.error, run.host.log);
    let r = run.report();
    assert!(r.trades > 0, "no trades: {:?}", &run.host.log[..run.host.log.len().min(20)]);
}

#[test]
fn rsi_ea_runs() {
    let src = SAMPLES.iter().find(|(n, _)| n.contains("RSI Reversal")).unwrap().1;
    let run = run_ea("rsi.mq5", src);
    assert!(run.error.is_none(), "{:?}", run.error);
}

#[test]
fn indicators_fill_buffers() {
    for (name, src) in SAMPLES.iter().filter(|(n, _)| n.starts_with("Indicators/")).chain([("t1.mq5", TEMPLATE_INDICATOR)].iter()) {
        let prog = Rc::new(compile(name, src, &no_loader).unwrap());
        let mut m = Market::new();
        let bars = m.bars(0, Timeframe::H1).to_vec();
        let spec = m.symbols[0].spec.clone();
        let mut run = TestRun::new(prog.clone(), spec, Timeframe::H1, bars.clone(), TickMode::OpenPrices, 10000.0, 100, &HashMap::new(), Box::new(|n| Err(format!("no {}", n))));
        run.host.view = bars.clone();
        let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
        inst.init(&mut run.host).unwrap();
        inst.calculate(&mut run.host).unwrap_or_else(|e| panic!("{}: {}", name, inst.fmt_err(&e)));
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
    let mut m = Market::new();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(prog.clone(), spec, Timeframe::H1, bars, TickMode::OpenPrices, 10000.0, 100, &HashMap::new(), Box::new(|n| Err(format!("no {}", n))));
    let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
    inst.init(&mut run.host).unwrap();
    inst.call_event("OnStart", vec![], &mut run.host).unwrap_or_else(|e| panic!("{}", inst.fmt_err(&e)));
    let last = run.host.log.last().unwrap().clone();
    assert!(last.ends_with("3 3.5 x3 3 112 55 2 9 003.1|ab |42"), "{}", last);
}

#[test]
fn runtime_errors() {
    let src = "void OnStart(){ int z = 0; int a[3]; int x = 5 / z; }";
    let prog = Rc::new(compile("err.mq5", src, &no_loader).unwrap());
    let mut m = Market::new();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(prog.clone(), spec, Timeframe::H1, bars, TickMode::OpenPrices, 10000.0, 100, &HashMap::new(), Box::new(|n| Err(format!("no {}", n))));
    let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
    inst.init(&mut run.host).unwrap();
    let e = inst.call_event("OnStart", vec![], &mut run.host).unwrap_err();
    assert_eq!(e.msg, "zero divide");
}

/// Compile and run a script's OnStart, returning what it printed.
fn run_script(src: &str) -> Vec<String> {
    let prog = Rc::new(compile("script.mq5", src, &no_loader).unwrap_or_else(|e| panic!("{:?}", e)));
    let mut m = Market::new();
    let bars = m.bars(0, Timeframe::H1).to_vec();
    let spec = m.symbols[0].spec.clone();
    let mut run = TestRun::new(prog.clone(), spec, Timeframe::H1, bars, TickMode::OpenPrices, 10000.0, 100, &HashMap::new(), Box::new(|n| Err(format!("no {}", n))));
    let mut inst = crate::mql::Instance::new(prog, "EURUSD", Timeframe::H1, &HashMap::new());
    inst.init(&mut run.host).unwrap_or_else(|e| panic!("{}", inst.fmt_err(&e)));
    inst.call_event("OnStart", vec![], &mut run.host).unwrap_or_else(|e| panic!("{}", inst.fmt_err(&e)));
    // log lines look like "time\tprogram\tmessage" or similar: keep only the message text
    run.host.log.iter().map(|l| l.rsplit('\t').next().unwrap_or(l).to_string()).collect()
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
    assert!(joined.contains("rect=6.0 square=16.0 22 16 2 true 8 2.5 hi true true true"), "{}", joined);
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
    let mut m = Market::new();
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
            [base.join(p), inc.join(p)].iter().find_map(|c| std::fs::read(c).ok()).map(|b| crate::storage::decode_source(&b))
        };
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let prog = match compile(&name, &src, &loader) {
            Ok(p) => Rc::new(p),
            Err(e) => {
                let first = e.first().map(|e| format!("{} (line {})", e.msg, e.line)).unwrap_or_default();
                let key = first.split(" (line").next().unwrap_or("").to_string();
                let key = key.split(" - ").last().unwrap_or(&key).to_string();
                println!("COMPILE FAIL {}: {}", rel, first);
                errs.entry(format!("compile: {}", key)).or_default().push(rel);
                continue;
            }
        };
        ok_c += 1;
        let ind_root2 = ind_root.clone();
        let inc2 = inc_root.clone();
        let prog_loader: Box<dyn Fn(&str) -> Result<Rc<crate::mql::Program>, String>> = Box::new(move |n: &str| {
            let rel = n.replace('\\', "/");
            let rel = if rel.ends_with(".mq5") { rel } else { format!("{}.mq5", rel) };
            let p = ind_root2.join(&rel);
            let b = std::fs::read(&p).map_err(|e| format!("{}: {}", rel, e))?;
            let src = crate::storage::decode_source(&b);
            let base = p.parent().unwrap().to_path_buf();
            let inc = inc2.clone();
            let l = move |q: &str| -> Option<String> { [base.join(q), inc.join(q)].iter().find_map(|c| std::fs::read(c).ok()).map(|b| crate::storage::decode_source(&b)) };
            compile(&rel, &src, &l).map(Rc::new).map_err(|e| format!("{:?}", e.first()))
        });
        let result: Result<(), String> = (|| {
            match prog.kind {
                crate::mql::ProgramKind::Indicator => {
                    let mut run = TestRun::new(prog.clone(), spec.clone(), Timeframe::H1, bars.clone(), TickMode::OpenPrices, 10000.0, 100, &HashMap::new(), prog_loader);
                    run.host.view = bars.clone();
                    let mut inst = crate::mql::Instance::new(prog.clone(), "EURUSD", Timeframe::H1, &HashMap::new());
                    let code = inst.init(&mut run.host).map_err(|e| inst.fmt_err(&e))?;
                    if code != 0 {
                        return Err(format!("OnInit returned {}", code));
                    }
                    inst.calculate(&mut run.host).map_err(|e| inst.fmt_err(&e))?;
                    inst.calculate(&mut run.host).map_err(|e| inst.fmt_err(&e))?;
                    Ok(())
                }
                crate::mql::ProgramKind::Expert => {
                    let short: Vec<crate::market::Bar> = bars[..600.min(bars.len())].to_vec();
                    let mut run = TestRun::new(prog.clone(), spec.clone(), Timeframe::H1, short, TickMode::OpenPrices, 10000.0, 100, &HashMap::new(), prog_loader);
                    let mut guard = 0;
                    while !run.step(100) && guard < 1000 {
                        guard += 1;
                    }
                    match &run.error {
                        Some(e) => Err(e.clone()),
                        None => Ok(()),
                    }
                }
                _ => Ok(()),
            }
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
    println!("\n==== {} files: {} compile, {} compile+run OK ====", files.len(), ok_c, ok_r);
    for (k, f) in v.iter().take(60) {
        println!("{:4}  {}", f.len(), k);
    }
}
