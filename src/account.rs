//! Simulated trading account: market and pending orders, positions (hedging), deals,
//! margin, stop loss / take profit and stop out.

use crate::market::{fmt_price, ProfitCalc, SymbolSpec};

pub const RET_DONE: i64 = 10009;
pub const RET_REJECT: i64 = 10006;
pub const RET_INVALID: i64 = 10013;
pub const RET_INVALID_VOLUME: i64 = 10014;
pub const RET_INVALID_PRICE: i64 = 10015;
pub const RET_INVALID_STOPS: i64 = 10016;
pub const RET_TRADE_DISABLED: i64 = 10017;
pub const RET_NO_MONEY: i64 = 10019;
pub const RET_POSITION_CLOSED: i64 = 10036;

/// Stop out level in percent of margin (estimated, typical broker default).
pub const STOP_OUT_LEVEL: f64 = 50.0;
pub const MARGIN_CALL_LEVEL: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OrderType {
    Buy = 0,
    Sell = 1,
    BuyLimit = 2,
    SellLimit = 3,
    BuyStop = 4,
    SellStop = 5,
}

impl OrderType {
    pub fn from_i64(v: i64) -> Option<OrderType> {
        Some(match v {
            0 => OrderType::Buy,
            1 => OrderType::Sell,
            2 => OrderType::BuyLimit,
            3 => OrderType::SellLimit,
            4 => OrderType::BuyStop,
            5 => OrderType::SellStop,
            _ => return None,
        })
    }
    pub fn is_buy(self) -> bool {
        matches!(self, OrderType::Buy | OrderType::BuyLimit | OrderType::BuyStop)
    }
    pub fn label(self) -> &'static str {
        match self {
            OrderType::Buy => "buy",
            OrderType::Sell => "sell",
            OrderType::BuyLimit => "buy limit",
            OrderType::SellLimit => "sell limit",
            OrderType::BuyStop => "buy stop",
            OrderType::SellStop => "sell stop",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub ticket: u64,
    pub symbol: String,
    pub buy: bool,
    pub volume: f64,
    pub price_open: f64,
    pub sl: f64,
    pub tp: f64,
    pub time: i64,
    pub magic: i64,
    pub comment: String,
    pub price_current: f64,
    pub profit: f64,
    pub digits: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingOrder {
    pub ticket: u64,
    pub symbol: String,
    pub typ: OrderType,
    pub volume: f64,
    pub price: f64,
    pub sl: f64,
    pub tp: f64,
    pub time: i64,
    pub magic: i64,
    pub comment: String,
    pub digits: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Deal {
    pub ticket: u64,
    pub order: u64,
    pub position: u64,
    pub time: i64,
    pub symbol: String,
    /// "buy", "sell" or "balance"
    pub kind: String,
    /// "in", "out" or ""
    pub entry: String,
    pub volume: f64,
    pub price: f64,
    pub profit: f64,
    pub magic: i64,
    pub comment: String,
    pub digits: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub login: u64,
    pub name: String,
    pub server: String,
    pub currency: String,
    pub leverage: u32,
    pub balance: f64,
    pub positions: Vec<Position>,
    pub orders: Vec<PendingOrder>,
    pub deals: Vec<Deal>,
    pub next_ticket: u64,
    #[serde(default)]
    pub password: String,
}

/// Prices for one symbol.
#[derive(Clone)]
pub struct Quote {
    pub spec: SymbolSpec,
    pub bid: f64,
    pub ask: f64,
}

pub trait QuoteSource {
    fn quote(&self, symbol: &str) -> Option<Quote>;
}

pub struct TradeResult {
    pub retcode: i64,
    pub order: u64,
    pub deal: u64,
    pub price: f64,
    pub volume: f64,
    pub comment: String,
}

impl TradeResult {
    fn fail(retcode: i64, comment: &str) -> Self {
        TradeResult { retcode, order: 0, deal: 0, price: 0.0, volume: 0.0, comment: comment.to_string() }
    }
}

pub fn profit_usd(spec: &SymbolSpec, buy: bool, volume: f64, open: f64, close: f64, cur_bid: f64) -> f64 {
    let diff = if buy { close - open } else { open - close };
    let raw = diff * volume * spec.contract_size;
    match spec.calc {
        ProfitCalc::QuoteUsd => raw,
        ProfitCalc::BaseUsd => {
            if cur_bid > 0.0 {
                raw / cur_bid
            } else {
                raw
            }
        }
        ProfitCalc::TickValue { tick_value, tick_size } => {
            if tick_size > 0.0 {
                diff / tick_size * tick_value * volume
            } else {
                raw
            }
        }
    }
}

pub fn margin_usd(spec: &SymbolSpec, volume: f64, price: f64, leverage: u32) -> f64 {
    let lev = leverage.max(1) as f64;
    match spec.calc {
        ProfitCalc::QuoteUsd => volume * spec.contract_size * price / lev,
        ProfitCalc::BaseUsd => volume * spec.contract_size / lev,
        // quote currency converted to USD with the broker's tick value
        ProfitCalc::TickValue { tick_value, tick_size } => {
            let rate = if tick_size > 0.0 && spec.contract_size > 0.0 { tick_value / (tick_size * spec.contract_size) } else { 1.0 };
            volume * spec.contract_size * price * rate / lev
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

impl Account {
    pub fn new_demo(login: u64, name: &str, deposit: f64, leverage: u32) -> Self {
        let mut a = Account {
            login,
            name: name.to_string(),
            server: "Simulated-Demo".to_string(),
            currency: "USD".to_string(),
            leverage,
            balance: 0.0,
            positions: vec![],
            orders: vec![],
            deals: vec![],
            next_ticket: 2,
            password: String::new(),
        };
        a.deposit(deposit, "Deposit", 0);
        a
    }

    pub fn deposit(&mut self, amount: f64, comment: &str, time: i64) {
        let t = self.take_ticket();
        self.balance = round2(self.balance + amount);
        self.deals.push(Deal {
            ticket: t,
            order: 0,
            position: 0,
            time,
            symbol: String::new(),
            kind: "balance".into(),
            entry: String::new(),
            volume: 0.0,
            price: 0.0,
            profit: amount,
            magic: 0,
            comment: comment.into(),
            digits: 2,
        });
    }

    fn take_ticket(&mut self) -> u64 {
        let t = self.next_ticket;
        self.next_ticket += 1;
        t
    }

    pub fn floating(&self) -> f64 {
        self.positions.iter().map(|p| p.profit).sum()
    }
    pub fn equity(&self) -> f64 {
        round2(self.balance + self.floating())
    }
    pub fn margin(&self, quotes: &dyn QuoteSource) -> f64 {
        self.positions
            .iter()
            .map(|p| match quotes.quote(&p.symbol) {
                Some(q) => margin_usd(&q.spec, p.volume, p.price_open, self.leverage),
                None => 0.0,
            })
            .sum()
    }
    pub fn margin_level(&self, quotes: &dyn QuoteSource) -> f64 {
        let m = self.margin(quotes);
        if m <= 0.0 {
            0.0
        } else {
            self.equity() / m * 100.0
        }
    }

    fn check_volume(spec: &SymbolSpec, volume: f64) -> bool {
        volume >= spec.volume_min - 1e-9 && volume <= spec.volume_max + 1e-9
    }

    fn check_stops(buy: bool, price: f64, sl: f64, tp: f64) -> bool {
        if buy {
            (sl == 0.0 || sl < price) && (tp == 0.0 || tp > price)
        } else {
            (sl == 0.0 || sl > price) && (tp == 0.0 || tp < price)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn market_order(
        &mut self,
        quotes: &dyn QuoteSource,
        symbol: &str,
        buy: bool,
        volume: f64,
        sl: f64,
        tp: f64,
        magic: i64,
        comment: &str,
        time: i64,
        log: &mut Vec<String>,
    ) -> TradeResult {
        let q = match quotes.quote(symbol) {
            Some(q) => q,
            None => return TradeResult::fail(RET_INVALID, "unknown symbol"),
        };
        let volume = (volume / q.spec.volume_step).round() * q.spec.volume_step;
        if !Self::check_volume(&q.spec, volume) {
            log.push(format!("'{}': failed market {} {:.2} {} [Invalid volume]", self.login, if buy { "buy" } else { "sell" }, volume, symbol));
            return TradeResult::fail(RET_INVALID_VOLUME, "Invalid volume");
        }
        let price = if buy { q.ask } else { q.bid };
        if !Self::check_stops(buy, price, sl, tp) {
            log.push(format!("'{}': failed market {} {:.2} {} sl: {} tp: {} [Invalid stops]", self.login, if buy { "buy" } else { "sell" }, volume, symbol, fmt_price(sl, q.spec.digits), fmt_price(tp, q.spec.digits)));
            return TradeResult::fail(RET_INVALID_STOPS, "Invalid stops");
        }
        let need = margin_usd(&q.spec, volume, price, self.leverage);
        let free = self.equity() - self.margin(quotes);
        if need > free {
            log.push(format!("'{}': failed market {} {:.2} {} [No money]", self.login, if buy { "buy" } else { "sell" }, volume, symbol));
            return TradeResult::fail(RET_NO_MONEY, "No money");
        }
        let order = self.take_ticket();
        let deal = self.take_ticket();
        log.push(format!(
            "'{}': market {} {:.2} {} sl: {} tp: {}",
            self.login,
            if buy { "buy" } else { "sell" },
            volume,
            symbol,
            fmt_price(sl, q.spec.digits),
            fmt_price(tp, q.spec.digits)
        ));
        log.push(format!(
            "'{}': deal #{} {} {:.2} {} at {} done (based on order #{})",
            self.login,
            deal,
            if buy { "buy" } else { "sell" },
            volume,
            symbol,
            fmt_price(price, q.spec.digits),
            order
        ));
        self.positions.push(Position {
            ticket: order,
            symbol: q.spec.name.to_string(),
            buy,
            volume,
            price_open: price,
            sl,
            tp,
            time,
            magic,
            comment: comment.to_string(),
            price_current: if buy { q.bid } else { q.ask },
            profit: 0.0,
            digits: q.spec.digits,
        });
        self.deals.push(Deal {
            ticket: deal,
            order,
            position: order,
            time,
            symbol: q.spec.name.to_string(),
            kind: if buy { "buy" } else { "sell" }.into(),
            entry: "in".into(),
            volume,
            price,
            profit: 0.0,
            magic,
            comment: comment.into(),
            digits: q.spec.digits,
        });
        self.refresh(quotes);
        TradeResult { retcode: RET_DONE, order, deal, price, volume, comment: "Request executed".into() }
    }

    pub fn close_position(
        &mut self,
        quotes: &dyn QuoteSource,
        ticket: u64,
        volume: Option<f64>,
        time: i64,
        reason: &str,
        log: &mut Vec<String>,
    ) -> TradeResult {
        let idx = match self.positions.iter().position(|p| p.ticket == ticket) {
            Some(i) => i,
            None => return TradeResult::fail(RET_POSITION_CLOSED, "Position doesn't exist"),
        };
        let pos = self.positions[idx].clone();
        let q = match quotes.quote(&pos.symbol) {
            Some(q) => q,
            None => return TradeResult::fail(RET_INVALID, "unknown symbol"),
        };
        let vol = volume.unwrap_or(pos.volume).min(pos.volume);
        if vol <= 0.0 {
            return TradeResult::fail(RET_INVALID_VOLUME, "Invalid volume");
        }
        let price = if pos.buy { q.bid } else { q.ask };
        let profit = round2(profit_usd(&q.spec, pos.buy, vol, pos.price_open, price, q.bid));
        let order = self.take_ticket();
        let deal = self.take_ticket();
        self.balance = round2(self.balance + profit);
        log.push(format!(
            "'{}': {}close position #{} {} {:.2} {} at {} profit {:.2}",
            self.login,
            reason,
            ticket,
            if pos.buy { "buy" } else { "sell" },
            vol,
            pos.symbol,
            fmt_price(price, q.spec.digits),
            profit
        ));
        self.deals.push(Deal {
            ticket: deal,
            order,
            position: ticket,
            time,
            symbol: pos.symbol.clone(),
            kind: if pos.buy { "sell" } else { "buy" }.into(),
            entry: "out".into(),
            volume: vol,
            price,
            profit,
            magic: pos.magic,
            comment: if reason.is_empty() { pos.comment.clone() } else { reason.trim().to_string() },
            digits: pos.digits,
        });
        if vol >= pos.volume - 1e-9 {
            self.positions.remove(idx);
        } else {
            self.positions[idx].volume = ((pos.volume - vol) * 100.0).round() / 100.0;
        }
        self.refresh(quotes);
        TradeResult { retcode: RET_DONE, order, deal, price, volume: vol, comment: "Request executed".into() }
    }

    pub fn modify_position(&mut self, quotes: &dyn QuoteSource, ticket: u64, sl: f64, tp: f64, log: &mut Vec<String>) -> TradeResult {
        let login = self.login;
        let p = match self.positions.iter_mut().find(|p| p.ticket == ticket) {
            Some(p) => p,
            None => return TradeResult::fail(RET_POSITION_CLOSED, "Position doesn't exist"),
        };
        let q = match quotes.quote(&p.symbol) {
            Some(q) => q,
            None => return TradeResult::fail(RET_INVALID, "unknown symbol"),
        };
        let price = if p.buy { q.bid } else { q.ask };
        if !Self::check_stops(p.buy, price, sl, tp) {
            return TradeResult::fail(RET_INVALID_STOPS, "Invalid stops");
        }
        p.sl = sl;
        p.tp = tp;
        log.push(format!("'{}': modify position #{} {} sl: {} tp: {}", login, ticket, p.symbol, fmt_price(sl, p.digits), fmt_price(tp, p.digits)));
        TradeResult { retcode: RET_DONE, order: ticket, deal: 0, price, volume: p.volume, comment: "Request executed".into() }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pending_order(
        &mut self,
        quotes: &dyn QuoteSource,
        symbol: &str,
        typ: OrderType,
        volume: f64,
        price: f64,
        sl: f64,
        tp: f64,
        magic: i64,
        comment: &str,
        time: i64,
        log: &mut Vec<String>,
    ) -> TradeResult {
        let q = match quotes.quote(symbol) {
            Some(q) => q,
            None => return TradeResult::fail(RET_INVALID, "unknown symbol"),
        };
        if !Self::check_volume(&q.spec, volume) {
            return TradeResult::fail(RET_INVALID_VOLUME, "Invalid volume");
        }
        let ok_price = match typ {
            OrderType::BuyLimit => price < q.ask,
            OrderType::SellLimit => price > q.bid,
            OrderType::BuyStop => price > q.ask,
            OrderType::SellStop => price < q.bid,
            _ => false,
        };
        if !ok_price || price <= 0.0 {
            log.push(format!("'{}': failed {} {:.2} {} at {} [Invalid price]", self.login, typ.label(), volume, symbol, fmt_price(price, q.spec.digits)));
            return TradeResult::fail(RET_INVALID_PRICE, "Invalid price");
        }
        if !Self::check_stops(typ.is_buy(), price, sl, tp) {
            return TradeResult::fail(RET_INVALID_STOPS, "Invalid stops");
        }
        let ticket = self.take_ticket();
        log.push(format!("'{}': {} {:.2} {} at {} placed #{}", self.login, typ.label(), volume, symbol, fmt_price(price, q.spec.digits), ticket));
        self.orders.push(PendingOrder {
            ticket,
            symbol: q.spec.name.to_string(),
            typ,
            volume,
            price,
            sl,
            tp,
            time,
            magic,
            comment: comment.to_string(),
            digits: q.spec.digits,
        });
        TradeResult { retcode: RET_DONE, order: ticket, deal: 0, price, volume, comment: "Request executed".into() }
    }

    pub fn delete_order(&mut self, ticket: u64, log: &mut Vec<String>) -> TradeResult {
        match self.orders.iter().position(|o| o.ticket == ticket) {
            Some(i) => {
                let o = self.orders.remove(i);
                log.push(format!("'{}': cancel order #{} {} {:.2} {}", self.login, ticket, o.typ.label(), o.volume, o.symbol));
                TradeResult { retcode: RET_DONE, order: ticket, deal: 0, price: o.price, volume: o.volume, comment: "Request executed".into() }
            }
            None => TradeResult::fail(RET_INVALID, "Order doesn't exist"),
        }
    }

    pub fn modify_order(&mut self, ticket: u64, price: f64, sl: f64, tp: f64) -> TradeResult {
        match self.orders.iter_mut().find(|o| o.ticket == ticket) {
            Some(o) => {
                if price > 0.0 {
                    o.price = price;
                }
                o.sl = sl;
                o.tp = tp;
                TradeResult { retcode: RET_DONE, order: ticket, deal: 0, price: o.price, volume: o.volume, comment: "Request executed".into() }
            }
            None => TradeResult::fail(RET_INVALID, "Order doesn't exist"),
        }
    }

    /// Recompute current prices and floating profit.
    pub fn refresh(&mut self, quotes: &dyn QuoteSource) {
        for p in self.positions.iter_mut() {
            if let Some(q) = quotes.quote(&p.symbol) {
                let cur = if p.buy { q.bid } else { q.ask };
                p.price_current = cur;
                p.profit = round2(profit_usd(&q.spec, p.buy, p.volume, p.price_open, cur, q.bid));
            }
        }
    }

    /// Per-tick processing: refresh, SL/TP hits, pending order activation and stop out.
    /// Returns true when something traded.
    pub fn process(&mut self, quotes: &dyn QuoteSource, time: i64, log: &mut Vec<String>) -> bool {
        self.refresh(quotes);
        let mut traded = false;
        // stop loss / take profit
        let hits: Vec<(u64, &'static str)> = self
            .positions
            .iter()
            .filter_map(|p| {
                let c = p.price_current;
                if p.buy {
                    if p.sl > 0.0 && c <= p.sl {
                        return Some((p.ticket, "[sl] "));
                    }
                    if p.tp > 0.0 && c >= p.tp {
                        return Some((p.ticket, "[tp] "));
                    }
                } else {
                    if p.sl > 0.0 && c >= p.sl {
                        return Some((p.ticket, "[sl] "));
                    }
                    if p.tp > 0.0 && c <= p.tp {
                        return Some((p.ticket, "[tp] "));
                    }
                }
                None
            })
            .collect();
        for (t, why) in hits {
            self.close_position(quotes, t, None, time, why, log);
            traded = true;
        }
        // pending orders
        let triggered: Vec<u64> = self
            .orders
            .iter()
            .filter(|o| {
                quotes.quote(&o.symbol).is_some_and(|q| match o.typ {
                    OrderType::BuyLimit => q.ask <= o.price,
                    OrderType::SellLimit => q.bid >= o.price,
                    OrderType::BuyStop => q.ask >= o.price,
                    OrderType::SellStop => q.bid <= o.price,
                    _ => false,
                })
            })
            .map(|o| o.ticket)
            .collect();
        for t in triggered {
            if let Some(i) = self.orders.iter().position(|o| o.ticket == t) {
                let o = self.orders.remove(i);
                log.push(format!("'{}': order #{} {} {:.2} {} at {} triggered", self.login, o.ticket, o.typ.label(), o.volume, o.symbol, fmt_price(o.price, o.digits)));
                self.market_order(quotes, &o.symbol, o.typ.is_buy(), o.volume, o.sl, o.tp, o.magic, &o.comment, time, log);
                traded = true;
            }
        }
        // stop out
        loop {
            let m = self.margin(quotes);
            if m <= 0.0 || self.equity() / m * 100.0 >= STOP_OUT_LEVEL {
                break;
            }
            let worst = self
                .positions
                .iter()
                .min_by(|a, b| a.profit.partial_cmp(&b.profit).unwrap_or(std::cmp::Ordering::Equal))
                .map(|p| p.ticket);
            match worst {
                Some(t) => {
                    log.push(format!("'{}': Stop Out: margin level below {:.0}%", self.login, STOP_OUT_LEVEL));
                    self.close_position(quotes, t, None, time, "[so] ", log);
                    traded = true;
                }
                None => break,
            }
        }
        traded
    }
}
