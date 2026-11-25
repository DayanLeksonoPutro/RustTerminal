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
