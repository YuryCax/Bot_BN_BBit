# 📘 ТЕХНИЧЕСКОЕ ЗАДАНИЕ (ТЗ)
## Low-Latency Алготрейдинговая Система: Binance Futures → Bybit Spot/Perpetual
**Версия:** 2.2  
**Дата:** 08.07.2026  
**Язык разработки:** Rust 1.78+  
**Среда исполнения:** Linux (Ubuntu 22.04/24.04 LTS), `x86_64`/`aarch64`  
**Архитектура:** Trading bot — Rust (`Observer` → `Executor`); **Analyst** — offline-советник (§8.6, Фаза 2); **Фаза 3** — validated Analyst → интеграция или 2-й акк (§10.7). Старт: **2× t3.micro** (§2.4). **Ресурсы:** §2.7.

> **Changelog v2.2:** §10.7 **Фаза 3** — валидация Analyst, graduated auto-apply, путь A (интеграция) / B (отдельный Bybit-акк).  
> **Changelog v2.1:** §2.7 **принципы ресурсоёмкости** — RAM/CPU budget t3.micro, lean deps, WS limits, запрет перегруза hot path.  
> **Changelog v2.0:** §1.7 **экономическая модель** (lag/follow-through = edge); §1.8 роли Observer vs Analyst в PnL; **2–3 пары**, **×10**; §5.2 выход **Lag Convergence** + **Time Stop**; §7 фильтры lag/vol; §9.0 **Edge Research** до paper; §8.6 Analyst = regime filter (не hot path); §10.6 Фаза 0.  
> **Changelog v1.9:** старт **$300**, **2× t3.micro** Tokyo+Singapore; §1.5 масштаб пар без рефакторинга; §1.6 **реалистичные** цели доходности (не 2%/день).  
> **Changelog v1.8:** дорожная карта **Фаза 1 / Фаза 2** (§10.6); §8.6.9 **Proposal & Apply** (Telegram/Panel, one-click); §8.7 **Order Book DB** для паттернов; Analyst — фаза 2.  
> **Changelog v1.7:** §8.6 сервис **`analyst`** — offline ИИ-аналитик (не трейдер): наблюдение MA, прогноз направления, отчёты; §9.1 go/no-go; trade journal export.  
> **Changelog v1.6:** исправлен `D_min_net` (fees без ×Lev); §5.5 единый `effective_SL`; fee-BE унифицирован; `MICRO_OK` по Bybit; Safe-Mode поэтапный; cancel-all → auto re-place exchange stop; replay с delay 150 ms; MVP mono-node; futures-first для spot.  
> **Changelog v1.5:** цель «рост депозита, не слив»; §5.4 адаптивное SL/TP по метрикам Binance; §6.0 раздельные депозиты Spot/Futures; §6.3 fee-aware sizing (вход/TP с учётом комиссий); §8.5.9 раздельная остановка торговли и снятие ордеров Spot/Futures.  
> **Changelog v1.4:** добавлена §8.5 «Панель управления» — веб-UI для распределения капитала по парам (% от депозита), включения/остановки пар, отображения профита (realized/unrealized, по паре и суммарно).  
> **Changelog v1.3:** формализовано принятие решения о входе (Observer only), исправлена машина состояний SL/TP, уточнена стратегия (lead-lag/momentum), политика Spot Short, единицы измерения, Risk Engine hot/warm path, freshness 150 ms, exit-EMA по Bybit, fail-safe Spot, политика потерь UDP, `packet_version = 2`.  
> **Changelog v1.2:** добавлен динамический Take-Profit (§5.4) с трейлингом, частичной фиксацией и интеграцией со стоп-лоссом.  
> **Changelog v1.1:** исправлена синхронизация времени между узлами, устранены противоречия в структурах данных и конфиге, интегрированы модули §11, уточнена логика Long/Short и Spot/Futures.

---

## 1. Общие положения и цели системы

| Параметр | Спецификация |
|----------|--------------|
| **Назначение** | Автоматизированная **кросс-биржевая импульсная торговля (lead-lag / follow-through)** с минимальной задержкой. **Источник alpha:** измеренный **lag Binance→Bybit** — импульс на Binance Futures, Bybit **ещё не догнал**, вход на Bybit, выход при **схлопывании lag** или invalidation (§1.7, §5.2). Binance — сигналы и SL/TP-метрики; Bybit — исполнение и `MICRO_OK`. Basis filter (§4.2) отсекает gap > 0.05%. **Не** классический lag-arb уже открытого gap и **не** «наблюдение ради наблюдения». |
| **Главная цель** | **Доказать и эксплуатировать положительный net edge** после комиссий (§1.7, §6.3). Рост депозита, не слив. Бот **не торгует** без `net_edge_est > 0` и режима с достаточным follow-through (§7). В позиции — адаптивный SL/TP по Binance (§5.4). |
| **Приоритет рынков** | **Futures-first:** основной edge из‑за меньших комиссий и Long/Short. Spot — **фаза 2**, только после paper PF ≥ 1.3 на futures (§6.0, §9.1). |
| **Принцип разделения** | `Observer` (Токио) = сбор данных, фильтрация шума, расчёт метрик, **полная оценка условий входа §7**, генерация `MarketStatePacket` с `entry_valid` + `direction_bias`.<br>`Executor` (Сингапур) = freshness/dedup, **Risk Engine (только фильтры исполнения)**, маршрутизация Spot/Futures, исполнение, ведение позиции, **локальный EMA для exit-триггеров**, управление капиталом.<br>**Дублирование entry-логики (Z, D_exp, D_min) на Executor запрещено.** |
| **Технологический стек** | **Trading (Rust):** `tokio`, `simd-json`, `zenoh`, `postcard`, `crossbeam`, `tracing`, `prometheus`, `rustls`, `teloxide`, Control Panel (`axum`). **Analyst (§8.6, отдельный сервис):** Python 3.11+ / TypeScript, LLM API — **не** в Rust binary. **Ресурсоёмкость:** §2.7 (lean crates, budget t3.micro). |
| **Инфраструктура (старт)** | **2× t3.micro:** Observer `ap-northeast-1` (Tokyo) + Executor `ap-southeast-1` (Singapore), VPC Peering. ~$5–15/мес (free tier частично). См. §2.4, §2.5. |
| **Инфраструктура (scale)** | При росте пар/RAM → t3.small; при депозите $3k+ и PF → c7a.xlarge (§2.5). |
| **Стартовый депозит** | **$300 USDT** futures (Bybit); spot off. Allocation см. §6.0, §10.1. |
| **Стартовые пары** | **2–3** futures (`BTCUSDT`, `ETHUSDT`, опц. `SOLUSDT`); расширение до 35 **без рефакторинга** (§1.5, §3.4). |
| **Плечо (старт)** | **×10** (`default_leverage_futures`); max **×20** (§6.2). ×50 **запрещён** — комиссии и ликвидация уничтожают edge на $300. |
| **Допустимые инструменты** | Whitelist: 20–35 пар. Стартовый набор — `config.toml` / `symbols.toml`. **Добавление и остановка пар в runtime** — только через Панель управления (§8.5) с hot-reload; произвольная подписка без оператора запрещена. |
| **Ключевые ограничения** | One-way latency Токио→Сингапур P95 ≤ 80 мс, P99 ≤ 110 мс. Freshness drop > 150 ms. Hot path Risk Engine ≤ 10 мкс (§4.2). Проскальзывание входа ≤ 0.05%. Максимальный дневной DD: Spot ≤ 2%, Futures ≤ 1.5%. |
| **Этапы разработки** | **Ф0** Edge Research (§9.0) → **Ф1** бот + Panel → **Ф2** БД + Analyst + Apply (§8.6–8.7) → **Ф3** validated Analyst: интеграция или 2-й акк (§10.7). Ф2–Ф3 **не блокируют** live Ф1. |

### 1.1. Архитектурная схема

| Узел | Регион AWS | Роль | Ключевые задачи |
|------|------------|------|-----------------|
| **Observer** | `ap-northeast-1` (Токио) | Data & Signal | Binance Futures WS → парсинг → метрики → **entry decision §7** → `MarketStatePacket` → Zenoh |
| **Executor** | `ap-southeast-1` (Сингапур) | Execution & Risk | Приём пакетов → Risk Engine → Bybit Spot / Futures → Position Manager |

```
[ Binance Futures WS ]
          ↓ (10–30 ms)
[ Server A: Observer ] — AWS Tokyo (ap-northeast-1)
   • simd-json parser
   • RingBuffer + Welford/Z/EMA/ATR
   • Entry Engine (§7) → entry_valid + direction_bias
   • Zenoh Publisher (UDP)
          ↓ (50–80 ms P95 via AWS Backbone)
[ Server B: Executor ] — AWS Singapore (ap-southeast-1)
   • Zenoh Subscriber + Freshness Check (≤150 ms)
   • Risk Engine hot path (<10μs) + warm cache
   • Router → BybitSpotConnector / BybitFuturesConnector
   • Bybit V5 Private WS Execution
   • Position Manager (SL/TP state machine §5, Bybit EMA exits)
          ↓
[ Bybit API ] → Spot / USDT Perpetual (Long / Short*)
```
*Short на Spot — только при `spot_margin_enabled = true` (§4.3).

**Ключевые принципы:**
1. **Токио не знает про ордера** — не хранит позиции, баланс, статус исполнения.
2. **Сингапур не пересчитывает entry-метрики** — использует `entry_valid`, `direction_bias`, `d_exp`, `d_min` из пакета; локально считает только **Bybit EMA** для exit-триггеров (§5.3).
3. **Spot и Futures разделены на уровне коннекторов и депозитов** (§6.0): отдельные кошельки Bybit, отдельные лимиты капитала и команды остановки; маршрутизация через `symbols.toml`.
4. **Long и Short** — симметричная логика входа/выхода с инверсией условий (§7).

### 1.5. Масштабирование пар без рефакторинга (заложить в код с первого дня)

Старт **2–3 пары**, целевой whitelist **до 35**. Архитектура **не меняется** при добавлении пары — только config + hot-reload.

| Принцип | Реализация |
|---------|------------|
| **Фиксированный пул слотов** | `SymbolRegistry` с `MAX_SYMBOLS = 35`; массивы `[RingBuffer; MAX]`, `[Metrics; MAX]`, `[PositionSlot; MAX]` — аллокация **один раз** при старте |
| **symbol_id** | `u16` 1..35 из `symbols.toml`; пакет `MarketStatePacket.symbol_id` — индекс в пуле, не String в hot path |
| **Добавление пары** | Panel / `SubscribeSymbol` → занять свободный слот → WS subscribe → **без** перекомпиляции и без новых `Vec` в loop |
| **Отключение пары** | `enabled = false`; слот **не удаляется** (reuse после remove) |
| **WS шarding** | Коннекторы заранее на **max 8 пар/conn × 6 conn = 48** — покрывает 35 с запасом |
| **Конфиг** | `symbols.toml` + runtime reload; валидация `active_count ≤ MAX_SYMBOLS` |

> **Запрещено в v1:** `HashMap<String, …>` в hot path; динамический `push` RingBuffer при каждой новой паре; hardcode списка из 3 символов в коде.

### 1.6. Реалистичные цели доходности (депозит $300, год)

> **2% в день — не цель и не KPI проекта.** Это ~730% годовых без просадок; ни одна стабильная retail-стратегия так не работает.

| Метрика | Реалистично (хороший исход) | Нереалистично |
|---------|----------------------------|---------------|
| **Net / день** | 0.05–0.3% ($0.15–0.90) в активные дни | **2%/день** ($6) каждый день |
| **Net / месяц** | 2–8% ($6–24) после paper PF ≥ 1.2 | 60%/мес (2%×30) |
| **Net / год** | 15–40% ($45–120) при дисциплине и без крупных DD | 1000%+ |

**Учёт расходов за год (старт 2× t3.micro):**

| Статья | ~/год |
|--------|-------|
| AWS 2× t3.micro | $60–180 |
| Депозит $300, +25% net/год | +$75 |
| **Итого ориентир** | **+$0–50** после инфры при удачном сценарии |

**KPI проекта (go/no-go):** `net_edge > 0` на отфильтрованных сигналах (§9.0), PF ≥ 1.2, max DD < 10% за paper, **положительный net за квартал** — не «2% каждый день».

### 1.7. Экономическая модель заработка (источник edge)

> **Наблюдение Binance не зарабатывает деньги.** Зарабатывает **повторяющийся lag**: Binance сдвинулся, Bybit **ещё не догнал**, вы входите на Bybit и выходите, когда догонялка **завершилась или сломалась**.

**Формула PnL системы:**

```
Net PnL = Σ (edge_per_trade − fees − slippage − funding)   по всем сделкам
edge_per_trade ≈ f(lag_residual, follow_through, exit_timing)
```

| Понятие | Определение | Где измеряется |
|---------|-------------|----------------|
| **Lag** | Расхождение mid Binance vs mid Bybit после импульса Binance | Observer + Executor (§3.5, §9.0) |
| **Follow-through** | Доля случаев: Bybit движется **в сторону** импульса Binance в окне 200–1000 ms | Edge Research + replay (§9.0) |
| **Net edge** | Условный return Bybit после сигнала **минус** round-trip fee **минус** типичный slippage | Heatmap по часам/vol (§9.0) |
| **Convergence** | Bybit догнал **70–90%** импульса Binance (`lag_capture_ratio`) | Exit trigger §5.2 |

**Три режима, где edge существует (торговать):**

| Режим | Условие |
|-------|---------|
| **High vol** | `ATR/price` выше медианы за 24 h — lag шире |
| **Volume impulse** | Импульс Binance подтверждён объёмом (`@aggTrade`), не один тик |
| **Lag ещё открыт** | `lag_residual > lag_min_bps` — Bybit **не** догнал импульс |

**Три режима, где edge = 0 (не торговать):**

| Режим | Причина |
|-------|---------|
| **Флэт / пила** | Follow-through < порога; комиссии съедают серию |
| **Lag схлопнулся до входа** | Опоздание — edge уже забрали |
| **Разворот Binance** | Thesis invalid — exit (§5.2 Invalidation) |

**Пример сделки ($300, alloc 10%, ×10):** маржа $30, notional $300; импulse +0.2% Binance, Bybit +0.15% → gross ~$0.45; fees ~$0.33 → **net ~$0.12**. Масштаб — **качество фильтров и число валидных сигналов**, не «смотреть чаще».

**Adaptive SL/TP (§5.4):** не создаёт edge; **улучшает распределение** (меньше отдали профит, режет хвост риска). **Не заменяет** доказательство follow-through.

### 1.8. Роли компонентов в заработке

```
БЫСТРО (мс)                         МЕДЛЕННО (мин–часы)
───────────                         ──────────────────
Binance WS → Observer (Rust)        Analyst (Фаза 2, offline)
  lag, Z, Vel, entry_valid            regime, alloc, tuning
  50–150 ms                           cron / по событию
       ↓                                    ↓
Executor → Bybit                         Suggestion → [Apply] → Operator
  исполнение, SL/TP, convergence
```

| Компонент | Создаёт edge? | Роль в деньгах |
|-----------|---------------|----------------|
| **Observer** | **Да** (hot path) | Ловит импульс, проверяет lag open, `entry_valid` |
| **Executor** | Нет | Исполнение, convergence exit, fee-aware sizing |
| **Analyst (ИИ)** | **Нет** | **Фильтр режима:** когда бот ON/OFF, alloc между 2–3 парами, tuning порогов; **Apply only** |
| **Operator** | Нет | Финальное «да» на предложения Analyst |

> **Запрещено:** LLM/Analyst в hot path входа; auto-apply; торговля без прохождения §9.0 Edge Research.

### 1.2. Принятие решения о входе (единственный источник — Observer)

```
[Tick Binance] → Noise Filter → Metrics (Z, Vel, EMA, ATR, regime)
       ↓
  Entry Engine (§7): D_exp, D_min_net, Z_threshold, regime matrix, **lag gates §3.5**
       ↓
  entry_valid = 1  ∧  lag open  ∧  direction_bias ∈ {-1, +1}  →  публикация пакета
  иначе            →  entry_valid = 0, direction_bias = 0
       ↓
[Executor] Freshness + Dedup + Risk Engine (§4.2)
       ↓
  entry_valid = 1  ∧  все risk-флаги OK  →  open_position(direction_bias)
  иначе            →  RISK_SKIP / drop
```

**Executor не вызывает формулы §7 для входа.** Поля `d_exp`, `d_min`, `sigma` в пакете — для аудита, логов и метрик, не для пересчёта.

### 1.3. Логическая цепочка (Binance → Bybit)

```
НАБЛЮДЕНИЕ     АНАЛИЗ + РЕШЕНИЕ          ДЕЙСТВИЕ              СОПРОВОЖДЕНИЕ
(Binance WS)   (Observer)               (Executor → Bybit)    (Executor)
     │              │                          │                    │
  aggTrade      Z, Vel, EMA, ATR         Risk Engine           SL/TP §5.4→§5.5
  bookTicker    Entry Engine §7          open / close          (метрики Binance)
  depth         entry_valid              Spot / Futures        + триггеры Bybit §5.2
                direction_bias
                     │
                     └── MarketStatePacket ──→ (freshness ≤150 ms)
```

**Правило:** Binance — **единственный источник решения о входе** и **адаптации SL/TP в позиции**. Bybit — **единственный источник цены исполнения** (mid, spread, depth) и **microstructure filter** для входа (`MICRO_OK`, §4.2).

### 1.4. Единицы измерения (обязательны для реализации)

| Величина | Единица | Пример |
|----------|---------|--------|
| **Цены** | USDT, `f64` | `67234.50` |
| **PnL** | Доля от entry (`pnl_pct`) | `0.005` = +0.5% |
| **Spread** | Доля от mid (`spread_pct`) | `0.0001` = 0.01% |
| **Velocity** | Доля цены **в секунду** | `(P_now − P_100ms) / P_100ms / 0.1`; `0.0001` ≈ 0.01%/s |
| **ATR, σ** | Абсолютные USDT или доля (`atr_pct = atr / price`) | ATR filter: `atr / price < 0.002` |
| **Z-Score** | Безразмерный | `2.5` |
| **Funding rate** | Доля | `0.0001` = 0.01% |
| **Latency** | Наносекунды UTC wall-clock | `150_000_000` = 150 ms |

---

## 2. Сетевая топология и требования к инфраструктуре

### 2.1. Размещение и соединения
- **Сервер A (Observer):** AWS `ap-northeast-1`. **Старт:** `t3.micro`. **Scale:** `c7a.xlarge` (§2.5).
- **Сервер B (Executor):** AWS `ap-southeast-1`. **Старт:** `t3.micro`. **Scale:** `c7a.xlarge`.
- **Сетевой мост:** `AWS VPC Peering` или `Transit Gateway`. Трафик между узлами идёт исключительно по внутренним IP через магистральную сеть AWS. Выход в публичный интернет разрешён только для API бирж, NTP, Telegram, Prometheus, Email.
- **Реальные метрики one-way latency** (`utc_now_ns() − packet.ts_ns` на Executor): P95: 50–80 мс, P99: 90–110 мс, Jitter: ≤ 5 мс.

### 2.2. Протокол межсерверного обмена
- **Библиотека:** `zenoh` v1.0+
- **Транспорт:** UDP (порт 7447), без гарантии доставки; **обязателен `seq_num`** в каждом пакете для детекции потерь и дедупликации.
- **Сериализация:** `postcard` + **версия схемы** `packet_version: u8` (текущая = **`3`** v2.0: поля lag §10.4). При изменении структуры — инкремент версии; узлы с несовместимой версией не стартуют.
- **Топик:** `market/binance/{symbol_id}`
- **Частота публикации:** 50–100 Гц в штатном режиме, до 500 Гц при `|Z| ≥ Z_threshold`.
- **Heartbeat:** Отдельный топик `system/heartbeat/tokyo`, пакет с `ts_ns` каждые 100 ms. Пропуски → поэтапный Safe-Mode (§5.2.1); emergency при timeout > 500 ms.
- **Таймстампы:** `ts_ns` = **UTC wall-clock** (`CLOCK_REALTIME`, наносекунды с эпохи). Синхронизация: `chrony` со stratum ≤ 2 на обоих узлах. **Запрещено** использовать `CLOCK_MONOTONIC` в межузловых пакетах.
- **Политика потерь UDP:**
  - **Dedup:** `seq_num <= last_seq_num[symbol_id]` → drop.
  - **Gap detection:** `seq_num > last + 1` → `seq_gap_count++`, лог `WARN`.
  - **Gap storm:** если `seq_gap_count > 10` за 1 с по символу → `pause_entries[symbol_id]` на 5 с, алерт.
  - Потерянные пакеты **не интерполируются**; следующий валидный пакет принимается как есть.
- **Safe-Mode RTT:** скользящий P95 one-way latency за 10 с > **150 ms** → Safe-Mode фаза 1 (§5.2.1). Измерение — `utc_now_ns() − packet.ts_ns`.

### 2.3. Настройки ОС и ядра (Linux Tuning)

**Observer (Токио):**
```bash
sysctl -w net.core.rmem_max=16777216
sysctl -w net.core.wmem_max=16777216
sysctl -w net.ipv4.tcp_timestamps=0
echo performance | tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
taskset -c 0,1,2,3 ./observer   # все ядра c7a.xlarge под Observer
```

**Executor (Сингапур):**
```bash
# те же sysctl и governor
taskset -c 0,1,2,3 ./executor   # все ядра c7a.xlarge под Executor
```

**Общие настройки сокетов (оба узла):**
```bash
TCP_NODELAY=1 для всех внешних подключений (Binance/Bybit)
SO_RCVBUF/SO_SNDBUF=131072 для Binance/Bybit сокетов
```

### 2.4. Стартовый deploy: 2× t3.micro (Tokyo + Singapore)

**Выбранная конфигурация старта** (депозит $300, **2–3 пары**, ×10, 1 год):

| Узел | Регион | Instance | Процесс |
|------|--------|----------|---------|
| Observer | `ap-northeast-1` Tokyo | **t3.micro** | `observer.service` |
| Executor + Panel | `ap-southeast-1` Singapore | **t3.micro** | `executor.service`, `control-panel.service` |

- Связь: Zenoh UDP + VPC Peering (§2.2).
- Пары на старте: `BTCUSDT`, `ETHUSDT` (+ опц. `SOLUSDT`).
- Spot: **off** (`spot_enabled = false`).
- Мониторинг: RAM & CPU credits; при OOM → апгрейд **Singapore** до t3.small первым.

### 2.5. Эволюция инфраструктуры

| Этап | Условие | Инфра |
|------|---------|-------|
| **Старт** | Paper + live 1% | 2× t3.micro |
| **Scale A** | RAM/CPU throttle | SG → t3.small или оба t3.small |
| **Scale B** | 10+ пар, PF ≥ 1.2 | t3.medium / c6a.large |
| **Scale C** | Депозит $3k+, dual-node latency critical | 2× c7a.xlarge |

### 2.6. MVP mono-node (альтернатива, проще отладка)

Один сервер Singapore in-process — опционально для **локальной отладки** перед 2× t3.micro:

| Параметр | Mono-node | Старт 2× t3.micro |
|----------|-----------|-------------------|
| **Регион** | Singapore only | Tokyo + Singapore |
| **Сложность** | ниже | выше |
| **Binance latency** | хуже | лучше (Observer в Tokyo) |

> Paper go/no-go (§9.1) обязателен в **обеих** схемах.

### 2.7. Принципы ресурсоёмкости (t3.micro, lean Rust)

> **Принцип:** быстрый язык на hot path, **минимум** процессов и аллокаций. Перегруз железа = latency jitter = потеря edge. Масштаб — **после** метрик, не «на вырост» на старте.

#### 2.7.1. Бюджет железа (старт 2× t3.micro: 1 vCPU, 1 GiB RAM каждый)

| Узел | Процесс | RAM soft limit | RAM hard / OOM action |
|------|---------|----------------|------------------------|
| **Tokyo** | `observer` | **≤ 400 MiB** | > 480 MiB 5 min → alert; > 550 MiB → restart + disable `@depth` |
| **Singapore** | `executor` | **≤ 350 MiB** | > 420 MiB → alert |
| **Singapore** | `control-panel` | **≤ 120 MiB** | > 150 MiB → отдельный t3.small **только Panel** |
| **Singapore** | `telegram-alerts` | **≤ 50 MiB** | in-process с Panel **запрещён** — отдельный lightweight process |

**CPU (burstable t3):** sustained load **≤ 40%** одного vCPU на узле; > 60% 10 min → алерт «CPU credit burn». Не использовать все ядра `taskset` на micro — **1 worker thread** hot loop + 1 IO thread достаточно для 2–3 пар.

**Disk:** логи `.bin` на root EBS **≤ 2 GiB** rolling; старые → S3 или delete. PostgreSQL/TimescaleDB **не на t3.micro** (Фаза 2 → отдельный инстанс или managed DB).

#### 2.7.2. WebSocket и потоки данных (старт 2–3 пары)

| Параметр | Старт (`mode = start`) | Scale (`mode = scale`) |
|----------|------------------------|-------------------------|
| WS conn Binance (Observer) | **1–2** (все пары на одном conn) | до 6 (§3.1) |
| Потоки на пару | `@aggTrade`, `@bookTicker` | + `@depth10@100ms` |
| `@depth10@100ms` на старте | **off** (`depth_enabled = false`) | on при RAM headroom |
| Bybit WS (Executor) | **1** private + **1** public | sharding по §3.1 |
| Zenoh publish rate cap | **100 Hz**/symbol avg | 500 Hz burst |

> Depth отключён на старте: imbalance для входа — **Bybit** warm path (§4.2), не Binance depth10.

#### 2.7.3. Rust workspace — разделение и запреты

**Crates (Cargo workspace):**

```
observer-core/   # hot path only — минимальные deps
executor-core/
shared/          # postcard types, no async runtime
observer-bin/
executor-bin/
panel/             # axum — отдельный binary, не link в observer
```

**Разрешено в `observer-core` / `executor-core`:**

| Категория | Crates |
|-----------|--------|
| Parse/IO | `simd-json`, `tokio`, `tokio-tungstenite`, `rustls` |
| IPC | `zenoh`, `postcard` |
| Math/sync | `crossbeam`, atomics, fixed arrays |
| Observability | `tracing` (sampled), `prometheus` |

**Запрещено в hot-path crates** (`observer-core`, `executor-core`):

| Запрещено | Причина |
|-----------|---------|
| `sqlx`, `diesel`, `postgres`, Timescale client | БД — offline / sidecar |
| `reqwest` / HTTP client в tick loop | Блокирует или аллоцирует |
| `serde_json` full parse tick payloads | Только `simd-json` partial |
| `HashMap<String, _>` в hot loop | §1.5 SymbolRegistry |
| LLM / `openai` / Python embed | Analyst — Фаза 2 |
| `teloxide` в observer/executor binary | Telegram — sidecar |
| Debug log **каждого** tick | Только sample / event-driven |

**Release profile (обязательно):**

```toml
[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true
```

#### 2.7.4. Аллокации и hot path

- Pre-alloc at startup: `SymbolRegistry`, RingBuffers, packet pool — **zero alloc** после warmup (§3.4 proptest).
- `String` / `Vec` в publish loop — **запрещены**; только stack + pre-sized buffers.
- `tracing`: `INFO` default; `DEBUG` tick — только `RUST_LOG=debug` на staging ≤ 1 h.
- Batching Zenoh: ≤ 1 ms (§3.3), не micro-batching ради «красоты» с задержкой > 2 ms.

#### 2.7.5. Что **не** запускать на t3.micro

| Компонент | Где запускать |
|-----------|---------------|
| `book-collector` + TimescaleDB | Фаза 2: t3.small+ / RDS |
| `analyst` + LLM | Фаза 2: тот же или локальная машина оператора |
| Edge Research collector (§9.0) | **Локально** или 1 неделя на micro, затем stop |
| Prometheus server | External / Grafana Cloud free tier |
| Replay на полных `.bin` | Dev machine, не production micro |

#### 2.7.6. Алерты и scale triggers

| Метрика | Warning | Action |
|---------|---------|--------|
| `process_resident_memory_bytes` | > soft limit 5 min | Trim depth, reduce log rate |
| `cpu_usage_percent` | > 60% 10 min | Check WS fan-out; upgrade SG → t3.small |
| `zenoh_publish_p99_us` | > 500 µs | Profile alloc; audit deps |
| `cpu_credit_balance` (AWS) | low | Upgrade instance class |

Prometheus rules: §8.2 + `bot_resource_budget_exceeded`.

#### 2.7.7. Языки по слоям (итог)

| Слой | Язык | Latency budget |
|------|------|----------------|
| Observer Entry Engine | **Rust** | < 1 ms / packet |
| Executor Risk hot path | **Rust** | ≤ 10 µs |
| Panel / Telegram | **Rust** (отдельные bins) | не в tick path |
| Edge Research §9.0 | Python **или** Rust offline | дни, не мс |
| Analyst §8.6 | Python | минуты, cron |

---

## 3. Модуль Observer (Токио) — Сбор, парсинг, математика, Entry Engine

### 3.1. Подключения к Binance Futures
- Endpoint: `wss://fstream.binance.com/ws` (Paper: `wss://stream.binancefuture.com/ws`)
- Потоки на пару: `@aggTrade`, `@bookTicker`; `@depth10@100ms` — **только если** `depth_enabled = true` (§2.7.2, по умолчанию **false** на `mode = start`)
- Коннекторы: **старт** 1–2 WS (все пары на conn); **scale** до 4–6 conn × 5–8 пар (§2.7.2).
- **Старт (`t3.micro`):** `tokio` worker threads = **2** (1 IO + 1 compute); без `taskset` pinning.
- **Scale:** каждое соединение может быть привязано к ядру через `on_thread_start`.
- Reconnect: Exponential backoff (1s → 2s → 4s → max 30s), восстановление RingBuffer из последних сохранённых тиков.

> **Примечание:** Binance **Spot** не используется как источник данных. Сигналы генерируются по фьючерсному рынку Binance; исполнение — на Bybit Spot или Perpetual по маппингу в `symbols.toml`.

### 3.2. Парсинг и фильтрация шума
- Библиотека: `simd-json` (AVX2/NEON оптимизация)
- Partial Deserialization: Извлечение только полей `p` (price), `q` (quantity), `T` (trade time), `m` (isBuyerMaker), `b`/`a` (bookTicker).
- Zero-Copy: Данные передаются из сетевого буфера напрямую в аналитический контекст без `Vec::clone` или `String::from`.
- **Фильтры:**
  - `Volume Threshold`: `trade.volume_usd < $10,000` → игнор.
  - `Spread Validation`: Обновление цены принимается только если подтверждено `@aggTrade` или изменением `bookTicker` за < 50 мс.
  - `Clock Sync`: Сверка `Binance.event_time` (UTC ms) с локальным `CLOCK_REALTIME`. Отклонение > 50 мс → лог `WARN`, коррекция offset в `exchange_clock_offset_ns`.

### 3.3. Математическое ядро и Entry Engine
- **RingBuffer:** `Box<[f64; 2048]>` на пару. Lock-free запись/чтение (`crossbeam-queue` или атомарный индекс). Размер: ~16 КБ на пару.
- **Инкрементальный Z-Score:** Алгоритм Welford. $O(1)$ обновление среднего и дисперсии. Окно: 1000 тиков.
- **Velocity:** `(P_now − P_100ms_ago) / P_100ms_ago / 0.1` — **доля/с** (§1.3). Фильтр затухания: если $\frac{d^2P}{dt^2} < 0$, `d_exp` умножается на 0.7 перед сравнением с `D_min`.
- **Triple EMA:** EMA(50), EMA(200), EMA(500) по тикам. В пакет передаются `ema_50` и `ema_200`; EMA(500) используется локально для regime filter.
- **ATR(14):** Инкрементальный расчёт на тиках. Фильтр микро-флета: `ATR / price < 0.2%` → `entry_valid = 0`.
- **Regime Detection:** `TrendStrength = |EMA_50 - EMA_200| / ATR`. `regime: u8` (0=Range, 1=Transition, 2=Trend) — см. матрицу §7.
- **Микроструктурные метрики:** `bid_ask_imbalance`, `volume_delta_100ms` — из `@bookTicker` + `@aggTrade`; передаются в пакете **только для аудита**. Фильтр `MICRO_OK` на входе — по **Bybit** (§4.2 warm path), не по Binance delta.
- **Z_threshold (приоритет):**
  - `use_dynamic_thresholds = false` → `z_score_entry` из config (по умолчанию 2.5).
  - `use_dynamic_thresholds = true` → `Z_threshold = clamp(percentile(|Z|, 5000, 0.95) × 1.1, 1.8, 3.2)`; **`z_score_entry` игнорируется**.
- **Entry Engine (после метрик):** вычисляет `D_min_net`, `D_exp`, проверяет условия §7; выставляет `entry_valid`, `direction_bias`, записывает `d_exp`, `d_min`, `sigma`, `z_threshold_used` в пакет (`d_min` = `D_min_net`).
- **Публикация:** `MarketStatePacket` → `postcard` → Zenoh. Batching ≤ 1 мс.

### 3.4. SymbolRegistry (§1.5 — код)

```rust
pub const MAX_SYMBOLS: usize = 35;

pub struct SymbolRegistry {
    pub slots: [Option<SymbolConfig>; MAX_SYMBOLS],
    pub active_count: AtomicU16,
}

impl SymbolRegistry {
    pub fn add(&mut self, cfg: SymbolConfig) -> Result<u16, BotError>;
    pub fn set_enabled(&mut self, id: u16, enabled: bool);
}
```

- `add` → WS subscribe Binance + init pre-allocated RingBuffer в слоте.
- Proptest: add/disable до 35 пар, **zero alloc** в hot loop после warmup.

### 3.5. Lag Telemetry и Follow-through (Edge Research + runtime)

**Dual-feed:** Observer подписан на Binance; Executor (warm path) кэширует Bybit mid → Observer получает `bybit_mid_ref` через обратный канал `system/bybit_mid/{symbol_id}` (Zenoh, 10 Hz) **или** Executor дополняет пакет полем `bybit_mid` перед Risk (warm merge, не блокирует hot path Observer).

**На каждом тике / каждые 100 ms (per symbol):**

| Поле | Формула |
|------|---------|
| `binance_mid` | mid из `@bookTicker` |
| `bybit_mid` | mid Bybit Linear (warm cache) |
| `lag_bps` | `(binance_mid − bybit_mid) / bybit_mid × 10_000` |
| `impulse_bps_100ms` | `(binance_mid_now − binance_mid_100ms_ago) / binance_mid_100ms_ago × 10_000` |
| `lag_residual` | `impulse_bps_100ms − bybit_move_bps_since_impulse` — **сколько Bybit ещё не догнал** |

**Follow-through snapshot (offline + rolling 24 h):** при `|impulse_bps_100ms| ≥ impulse_min_bps` логировать forward returns Bybit на +200, +500, +1000 ms → Parquet / `.bin` event `FOLLOW_THROUGH`.

**Runtime gate (§7):** `entry_valid = 0` если `lag_residual < lag_min_bps` (lag уже схлопнулся) **или** rolling `follow_through_rate_1h < follow_through_min` (§10.1).

---

## 4. Модуль Executor (Сингапур) — Маршрутизация, исполнение, риск

### 4.1. Приём и валидация
- Async Zenoh Subscriber, неблокирующий `Stream`.
- Мгновенная десериализация в `MarketStatePacket`.
- **Freshness Check:**
  ```rust
  let latency_ns = utc_now_ns() - packet.ts_ns;
  if latency_ns > 150_000_000 { drop(packet); }  // > 150 ms (P99-safe)
  ```
  `utc_now_ns()` — UTC wall-clock на Executor, синхронизированный через chrony.
- **Dedup и gap:** см. §2.2.
- **Entry gate:** `entry_valid == 0` → пакет используется только для Position Manager (открытые позиции), **новый вход запрещён**.
- Протухшие пакеты игнорируются, лог `INFO`.

### 4.2. Risk Engine (hot path ≤ 10 мкс + warm cache)

Risk Engine разделён на два контура:

| Контур | SLA | Содержимое | Обновление |
|--------|-----|------------|------------|
| **Hot path** | ≤ 10 мкс | `entry_valid`, freshness OK, dedup OK, `RiskFlags` bitmap (precomputed) | Каждый пакет |
| **Warm path** | ≤ 1 ms (данные ≤ 100 ms stale OK) | capital, DD, correlation, Bybit spread/depth, funding, basis | Фоновые задачи |

**Warm path — фоновые задачи (не в hot path):**
- `@bookTicker` + `@aggTrade` Bybit WS → spread, depth, `bybit_mid`, **`bybit_volume_delta_100ms`** (каждый тик).
- `GET /v5/market/tickers?category=linear` → funding (интервал `ticker_poll_interval_sec`, по умолчанию 60 с).
- Balance / margin / DD — каждые 500 ms или по событию fill.

**RiskFlags bitmap (atomic u64, обновляется warm path):**

| Флаг | Условие блокировки входа |
|------|--------------------------|
| `CAPITAL_OK` | Достаточно капитала/маржи в слоте |
| `DD_OK` | DD Spot < 2%, Futures < 1.5% |
| `CORR_OK` | Суммарная экспозиция BTC+ETH одного направления ≤ 60% одного слота; при открытом BTC — ETH того же направления ≤ 30% слота |
| `BOOK_OK` | Bybit spread ≤ 0.01%, depth_10 ≥ $50k |
| `FUNDING_OK` (Futures) | `\|fundingRate\| ≤ 0.0001` или вход не против фандинга |
| `BASIS_OK` | `\|ref_price − bybit_mid\| / bybit_mid ≤ 0.0005` |
| `MICRO_OK` | Long: `bybit_volume_delta_100ms ≥ 0`; Short: `bybit_volume_delta_100ms ≤ 0` (**Bybit** aggTrade/book, warm path) |
| `SPOT_SIDE_OK` | Short на Spot только при `spot_margin_enabled` (§4.3) |
| `PAUSE_OK` | Нет `pause_entries` после gap storm (§2.2) |
| `PAIR_ENABLED` | Пара `enabled = true` в symbols.toml / панели (§8.5.3) |
| `ENTRIES_SPOT_OK` | `halt_entries_spot = false` (§8.5.9) |
| `ENTRIES_FUTURES_OK` | `halt_entries_futures = false` (§8.5.9) |
| `FEE_EDGE_OK` | `D_exp ≥ D_min_net` из пакета (§6.3) |

**Hot path pseudocode:**
```rust
if packet.entry_valid == 0 { return Skip; }
if !risk_flags.all_required() { log RISK_SKIP; return Skip; }
route_and_open(packet.direction_bias);
```

### 4.3. Маршрутизация, Spot Short и коннекторы
```rust
pub trait ExchangeConnector: Send + Sync {
    async fn open_position(&self, pkt: &MarketStatePacket, risk: RiskParams) -> Result<OrderResult, BotError>;
    async fn close_position(&self, pos_id: &str, reason: ExitReason) -> Result<(), BotError>;
    async fn update_stop(&self, pos_id: &str, new_price: f64) -> Result<(), BotError>;
    fn available_capital(&self) -> f64;
    fn instrument_type(&self) -> InstrumentType;
}

pub enum Side { Long, Short }
pub enum InstrumentType { Spot, Futures }
```

- Реализации: `BybitSpotConnector`, `BybitFuturesConnector`.
- Маппинг: `symbols.toml` → `{ instrument, leverage?, spot_margin_enabled? }`.

**Политика Spot Short:**

| Режим | `spot_margin_enabled` | Short на Spot |
|-------|----------------------|---------------|
| **По умолчанию** | `false` | **Запрещён.** `direction_bias = −1` для spot-символа → `SPOT_SIDE_OK = 0`, `RISK_SKIP`. |
| **Spot Margin** | `true` | Разрешён через Bybit Spot Margin (`/v5/spot-margin-trade/*`): borrow base → sell → repay on close. Требует отдельных API-permissions. |

- **Long (Spot/Futures):** `side = Buy`.
- **Short (Futures):** `side = Sell`, open short.
- **Short (Spot Margin):** borrow + sell base; close = buy + repay.

### 4.4. Логика исполнения
- Протокол: `Bybit V5 Private WebSocket`.
- **Pre-allocated order templates:** JSON-буфер и каноническая строка для HMAC pre-allocated at startup. **В момент ордера** (≤ 50 μs): вставка `timestamp_ms`, `symbol`, `side`, `qty` → пересчёт SHA256-HMAC. Полная pre-sign **невозможна** из-за timestamp.
- **Limit IOC + Fallback (по умолчанию):**
  1. `Limit IOC` по цене `mid ± 0.01%`.
  2. Таймер 50 мс. Не исполнился → `Market`.
  3. Проскальзывание `> 0.05%` → слот −20%, алерт.
- Конфиг-флаг `use_limit_fallback: false` → чистый Market.
- Подтверждение: `execution_report` по WS. Статус позиции обновляется в `PositionState`.

### 4.5. Fail-safe Spot (защита при сбое процесса)
- **При открытии позиции (Spot Long):** если `spot_exchange_stop = true` (config), выставляется **reduce-only Stop-Limit** на бирже на уровне `initial_sl × (1 − sl_exchange_buffer_pct)` (Long) с буфером 0.1%.
- **При каждом обновлении virtual SL:** если SL сдвинулся > 0.05% — amend exchange stop (debounce 1 s).
- **При штатном закрытии:** cancel exchange stop до market/limit close.
- **После `cancel-all orders` (§8.5.9):** если spot-позиция открыта → **немедленно** re-place exchange stop по текущему `effective_SL` (§5.5); алерт «stop restored».
- **Crash / SIGKILL:** exchange stop остаётся на бирже — единственная защита Spot при падении Executor.
- **Watchdog:** `systemd` `WatchdogSec=30`, `Restart=always`. При недоступности > 30 s + открытые позиции → Alertmanager `CRITICAL` (оператор вручную через `/flush` или биржу).

---

## 5. Управление позициями и логика выхода

### 5.0. Единая машина состояний SL / TP

Все пороги — `pnl_pct` от entry (§1.3). Обновление — каждый `MarketStatePacket` + `Bybit @bookTicker` mid для PnL и SL/TP trigger.

```
[OPEN]
  SL = Entry ∓ ATR×1.8          (initial)
  TP_fixed = Entry × (1 ± max(initial_target_pct, D_min_net))  (TP-0, §6.3)
       │
  PnL ≥ 0.15% ──→ SL-1: Entry ∓ ATR×0.5  (но не ниже fee-BE, §6.3)
       │
  PnL ≥ 0.30% ──→ SL-BE: Stop = fee-BE (Entry + round-trip fees)
       │           TP phase → TrailArm (начало трейлинга остатка)
       │
  Price crosses TP_fixed ──→ TP-0: закрыть 50% (partial_close_pct)
       │           SL остаётся fee-BE на остатке (§6.3)
       │           TP phase → DynamicTrail
       │
  DynamicTrail ──→ TP = Price ∓ ATR×K_tp (monotonic)
       │           Пересечение TP → закрыть остаток
       │
  [Optional TP-Extended] regime=Trend ∧ |Z|>2.0 (Futures only)
       └── K_tp × 1.5; выход также по Exhaustion / EMA Cross
```

| Порог PnL | SL | TP phase | Действие |
|-----------|-----|----------|----------|
| Open | Entry ∓ ATR×1.8 | Initial | TP_fixed установлен |
| ≥ 0.15% | Entry ∓ ATR×0.5 | Initial | — |
| ≥ 0.30% | fee-BE (§6.3) | TrailArm | Трейлинг TP активен, partial ещё нет |
| Cross TP_fixed | fee-BE (§6.3) | DynamicTrail | Partial 50%; SL/TP на остатке |
| Extended (opt.) | fee-BE или trail | Extended | Futures + Trend only |

**Long:** SL только повышается; TP trail — `max(TP_old, Price − ATR×K_tp)`.  
**Short:** SL только понижается; TP trail — `min(TP_old, Price + ATR×K_tp)`.

**Итоговый SL всегда через §5.5 (`effective_SL`)** — fee-BE, пороги PnL и Binance-adaptive не могут ослабить защиту.

### 5.5. Единое разрешение SL (`effective_SL`)

Три независимых источника предлагают уровень стопа; применяется **наиболее защитный** (для Long — максимальная цена SL):

```
sl_pnl      = SL из порогов PnL (§5.0: initial → SL-1 → fee-BE)
sl_binance  = SL из метрик Binance (§5.4: adaptive tighten)
sl_fee_be   = fee-BE (§6.3) — абсолютный пол

effective_SL_long  = max(sl_pnl, sl_binance, sl_fee_be)
effective_SL_short = min(sl_pnl, sl_binance, sl_fee_be)
```

- **Long:** SL только ↑ относительно предыдущего `effective_SL`.
- **Short:** SL только ↓.
- `current_stop` в `PositionState` = `effective_SL` после каждого пакета Observer + tick Bybit.
- Exchange stop (Spot §4.5, Futures §5.1) синхронизируется с `effective_SL`, не с промежуточными кандидатами.

### 5.1. Виртуальный стоп и синхронизация с биржей
- Стоп по умолчанию **виртуальный** (в памяти Executor).
- **Futures:** реальный `Stop-Market` на Bybit обязателен; amend каждые 5 с или при `ΔATR > 15%`. Ликвидационная цена локально; `Distance_to_Liq < 0.4%` → закрытие 50%.
- **Spot:** виртуальный SL + optional exchange Stop-Limit (§4.5). Safe-Mode — см. §5.2 (поэтапный, не мгновенный close).

### 5.2. Триггеры выхода

| Триггер | Условие | Источник данных | Действие |
|---------|---------|-----------------|----------|
| **Lag Convergence** | `lag_capture_ratio ≥ convergence_exit_ratio` (default 0.75): Bybit догнал ≥75% импульса Binance с момента entry | `lag_residual`, entry snapshot | Market close (full или partial per config) |
| **Time Stop** | Время в позиции > `time_stop_ms` (default 8000) **и** `lag_capture_ratio < 0.3` | Position timer + lag | Market close — edge не materialized |
| **Invalidation** | Binance `Vel` разворот против позиции **или** `impulse_bps_100ms` против `direction_bias` | Пакет Observer | Market close / SL → fee-BE |
| Stop-Loss | `bybit_mid` пересекает `effective_SL` (§5.5) | Bybit bookTicker | Market (Limit fallback §4.4) |
| Take-Profit | `bybit_mid` пересекает `current_tp` | Bybit bookTicker | Partial / full (§5.0) |
| Exhaustion | `\|Vel_binance\| < 0.00005` ∧ `\|Z_binance\| < 0.5` | Пакет Observer | Market close |
| EMA Cross | `bybit_ema_50` пересекает `bybit_ema_200` против позиции | **Executor local EMA on Bybit mid** | Market close |
| Spread Expansion | Bybit spread > 0.01% ∨ depth_10 < $50k | Bybit bookTicker | Limit ±0.02% |
| Safe-Mode | Heartbeat loss / P95 latency > 150 ms | §2.2, §5.2.1 | Поэтапно: halt → close |

**Приоритет:** Safe-Mode → Stop-Loss → **Lag Convergence** → Take-Profit → **Invalidation** → **Time Stop** → EMA Cross → Exhaustion → Spread Expansion.

#### 5.2.1. Safe-Mode (поэтапный, без panic-close на первом glitch)

| Фаза | Условие | Действие |
|------|---------|----------|
| **1 — Caution** | 1 пропуск heartbeat ИЛИ P95 latency > 150 ms | `halt_entries` spot + futures; позиции ведутся по SL/TP |
| **2 — Defensive** | 2–3 пропуска heartbeat подряд (`safe_mode_heartbeat_misses`, default 3) | Tighten all SL → `effective_SL = fee-BE` minimum |
| **3 — Emergency** | 5 пропусков ИЛИ heartbeat timeout > 500 ms | Market close **всех** позиций spot + futures |

> Фаза 1 не закрывает позиции — защита от ложных срабатываний на сетевом jitter.

> **Важно:** Entry-метрики (Z, Vel, EMA Binance) — из пакета Observer. **Exit EMA Cross** — только по Bybit mid на Executor (инкрементальный EMA(50/200), отдельный от Observer). Это **не дублирование entry-логики**.

### 5.3. Динамический Take-Profit (конфиг §10.1)

Take-Profit виртуальный; исполнение программное (§4.4). Фазы соответствуют §5.0:

| Фаза (`tp_phase`) | Код | Описание |
|-------------------|-----|----------|
| **Initial** | 0 | TP_fixed = Entry × (1 ± `initial_target_pct`) |
| **TrailArm** | 1 | PnL ≥ `trail_arm_pct` (0.3%): трейлинг активен, partial ещё нет |
| **DynamicTrail** | 2 | После partial close: TP = Price ∓ ATR × K_tp |
| **Extended** | 3 | Futures + Trend: K_tp × 1.5; Spot — **отключено** |

**Формулы:**
```
TP_fixed_long   = Entry × (1 + initial_target_pct)    // default 0.5%
TP_fixed_short  = Entry × (1 − initial_target_pct)

TP_trail_long   = CurrentPrice − ATR × K_tp_trail
TP_trail_short  = CurrentPrice + ATR × K_tp_trail

K_tp_trail = clamp(base_tp_trail_atr × (1 + 0.1 × |Z|), 0.8, 1.5)
```

**Связь TP ↔ SL (согласовано с §5.0):**

| Событие | SL | TP |
|---------|-----|-----|
| PnL ≥ 0.15% | SL-1 (Entry ∓ ATR×0.5), ≥ fee-BE | Initial |
| PnL ≥ 0.30% | fee-BE (§6.3) | TrailArm |
| TP-0 partial 50% | fee-BE на остатке | DynamicTrail |
| SL до TP-0 | Full close | Cancelled |
| `take_profit.enabled = false` | §5.0 SL only | Disabled |

### 5.4. Адаптивное поднятие SL/TP по метрикам Binance

**Смысл:** пока позиция открыта, Observer продолжает слать `MarketStatePacket` с актуальными Z, Velocity, ATR, regime с Binance Futures. Position Manager на Executor **на каждом пакете** пересчитывает SL/TP — не ждёт фиксированных порогов PnL, а реагирует на **положение дел на Binance**.

| Сигнал Binance (из пакета) | Действие по SL | Действие по TP |
|----------------------------|----------------|----------------|
| `\|Z\|` растёт, `\|Vel\|` в сторону позиции, regime=Trend | Поднять SL (Long) / опустить SL (Short) на `ΔATR × k_sl_tight` (k=0.3); не ниже fee-BE (§6.3) | Расширить TP trail: `K_tp × 1.2`; фаза → Extended при `\|Z\| > 2.0` |
| `\|Z\|` падает, `\|Vel\|` затухает (exhaustion) | Удержать текущий SL (не ослаблять) | Сузить TP trail; при `\|Vel\| < 0.00005` — досрочный выход (§5.2) |
| Regime → Range после Trend | SL → fee-BE минимум | Partial close 50% если PnL ≥ `D_min_net`; остаток — tight trail |
| Импульс против позиции (`Vel` разворот) | Немедленно SL → fee-BE или market close если PnL < 0 | Отменить Extended; ускорить выход |
| ATR вырос > 15% vs entry | Пересчитать SL distance = `ATR × 1.8` (не уже fee-BE) | Пересчитать TP trail distance |

**Правила монотонности (защита депозита):**
- **Long:** SL только ↑; TP trail только ↑ (не отдаём заработанное).
- **Short:** SL только ↓; TP trail только ↓.
- Поднятие SL **никогда** не опускает защиту ниже **fee-breakeven** (цена входа + round-trip комиссии, §6.3).
- Если Binance импульс сильный (`\|Z\| ≥ 2.5` ∧ `\|Vel\| > velocity_min`) — `sl_binance` → fee-BE **до** порога PnL 0.30% (опережающая защита); итог через §5.5.

**Источник ATR/Z/Vel/regime:** пакет Observer (Binance). **Trigger price:** Bybit mid (§5.2). **Итоговый SL:** §5.5.

---

## 6. Управление капиталом и мультиинструментальность

### 6.0. Раздельные депозиты Spot и Futures (Bybit)

**Да — у Spot и Futures на Bybit разные балансы.** Даже в Unified Trading Account (UTA) API возвращает **раздельный equity**:
- **Spot wallet** — USDT для спотовых сделок (`/v5/account/wallet-balance`, `accountType=UNIFIED`, coin USDT available for spot).
- **Derivatives wallet** — маржа USDT Perpetual (`totalMarginBalance`, `totalAvailableBalance` для linear).

Система **никогда не смешивает** эти пулы при расчёте размера позиции и allocation.

| Параметр | Spot | Futures |
|----------|------|---------|
| Баланс | `spot_equity_usdt` | `futures_equity_usdt` |
| Allocation пары | `spot_alloc_pct` — % от **spot** equity | `futures_alloc_pct` — % от **futures** equity |
| DD лимит | ≤ 2% от spot equity | ≤ 1.5% от futures equity |
| Max open positions | ≤ 3 | ≤ 2 |
| Остановка торговли | `halt_entries_spot` (§8.5.9) | `halt_entries_futures` (§8.5.9) |

- Обновление балансов: warm path Risk Engine, каждые 500 ms (`GET /v5/account/wallet-balance`).
- В панели — **два отдельных блока**: «Spot депозит» и «Futures депозит» с equity, allocation, PnL, кнопками stop/cancel.
- Перевод USDT Spot ↔ Futures **не автоматизируется** ботом; только вручную оператором на бирже (out of scope v1.6).

**Политика Spot (фаза 2):**
- Spot **отключён** в MVP (`deployment.spot_enabled = false`, §2.4).
- Включение spot-пар — только после futures paper PF ≥ `min_futures_pf_for_spot` (default 1.3).
- Spot-пары: `initial_target_pct` ≥ `spot_min_tp_pct` (default **0.008** = 0.8%) — иначе net edge после fees (§6.3) слишком мал при VIP0.
- Spot Long-only по умолчанию (§4.3) — половина Binance Short-сигналов недоступна.

### 6.1. Слоты и размер позиции
- Капитал пары = `wallet_equity × alloc_pct` (spot или futures — §6.0).
- Внутри лимита пары — до 5 слотов (по 20% от лимита пары) для одновременных входов **не используется** в v1.5; **одна позиция на пару**.
- **Формула размера (Futures):** `$Qty = \frac{WalletEquity \times AllocPct \times Risk\%}{ATR \times Multiplier}$`
- **Формула размера (Spot):** та же; `Lev = 1`.
- Келли: **только оффлайн** для калибровки `Risk%` и `Multiplier`.
- Динамическая адаптация: ликвидность недостаточна → объём уменьшается до `slippage ≤ 0.05%`.

### 6.3. Fee-aware sizing — торговля с учётом комиссий (не «кормить биржу»)

**Принцип:** каждая сделка должна иметь **положительное мат. ожидание после всех издержек**. Бот не открывает и не держит позицию, если net edge ≤ 0.

**Комиссии (конфиг `[fees]`, обновляются из Bybit VIP tier или вручную):**

| Рынок | Maker | Taker | По умолчанию (Bybit VIP0) |
|-------|-------|-------|---------------------------|
| Spot | `spot_maker_pct` | `spot_taker_pct` | 0.10% / 0.10% |
| Futures | `futures_maker_pct` | `futures_taker_pct` | 0.02% / 0.055% |

**Round-trip cost** (комиссии — от **notional**, leverage **не умножает** fee %):
```
fee_round_trip_spot    = spot_taker_pct × 2 + slippage_budget
fee_round_trip_futures = futures_taker_pct × 2 + slippage_budget
slippage_budget        = slippage_limit_pct (default 0.05%)
profit_buffer          = fee_profit_buffer_pct (default 0.03%)

D_min_net_spot    = fee_round_trip_spot + profit_buffer      # ≈ 0.28% VIP0
D_min_net_futures = fee_round_trip_futures + profit_buffer   # ≈ 0.19% VIP0
```

> **Исправлено v1.6:** формула `× Lev` для fees **удалена** — комиссия биржи считается от notional сделки, не от маржи.

**Минимальное движение для входа (Observer, §7):**
```
D_min_net = D_min_net_spot | D_min_net_futures   (по instrument пары)
Вход разрешён ⟺ D_exp ≥ D_min_net
```

**Fee-breakeven (безубыток с комиссиями, не «голый» entry):**
```
BE_long  = Entry × (1 + fee_round_trip)
BE_short = Entry × (1 − fee_round_trip)
```
SL-BE (§5.0, §5.4) устанавливается на **BE_long/BE_short**, а не на цену входа — иначе стоп в ноль = убыток на комиссиях.

**Минимальный Take-Profit:**
```
TP_min_long  = Entry × (1 + D_min_net)
TP_min_short = Entry × (1 − D_min_net)
```
`initial_target_pct` (§10.1) должен быть ≥ `D_min_net`; иначе конфиг невалиден при старте.

**В панели профит показывается двумя строками:**
- **Gross PnL** — без комиссий.
- **Net PnL** — минус накопленные fees (realized); **Net — основной показатель** для оценки «депозит растёт или сливается».

### 6.2. Лимиты и ограничения

- **Leverage:** старт **×10**; `max_leverage_futures = 20`. **×50 запрещён** в config и Risk Engine — при $300 round-trip fee ≈ 5.5% маржи на ×50 notional; один adverse 1% move ≈ −10% маржи.
- `MaxOpenPositions`: Spot ≤ 3, Futures ≤ 2.
- `Daily Drawdown`: Spot ≤ 2%, Futures ≤ 1.5%. При достижении → `stop_all`, алерт, блокировка до ручного сброса.
- **Correlation Limit (единая формулировка):** экспозиция BTC + ETH **одного направления** (Long или Short) ≤ **60% одного слота**. Если BTC Long открыт — новый ETH Long ≤ **30% слота**. Аналогично для Short.
- `Margin Call (Futures)`: `MarginRatio < 0.2` → аварийное закрытие всех фьючерсных позиций.

---

## 7. Формулы и алгоритмы (Observer — Entry Engine)

| Назначение | Формула | Константы / Условия |
|------------|---------|---------------------|
| **Z-Score** | `$Z = \frac{P_{curr} - \mu}{\sigma}$` | `\|Z\| ≥ Z_threshold` (§3.3) |
| **Z_threshold** | dynamic **или** `z_score_entry` | dynamic имеет **приоритет** при `use_dynamic_thresholds = true` |
| **Velocity** | `(P_now − P_100ms) / P_100ms / 0.1` | доля/с; Long: `> velocity_min`; Short: `< −velocity_min` |
| **Мин. движение (Spot)** | `$D_{min\_net} = fee_{round\_trip\_spot} + profit\_buffer$` | §6.3; ≈ 0.28% VIP0 |
| **Мин. движение (Futures)** | `$D_{min\_net} = fee_{round\_trip\_futures} + profit\_buffer$` | §6.3; ≈ 0.19% VIP0; **без ×Lev** |
| **Ожидаемое движение** | `$D_{exp} = \alpha \cdot \|Z\| \cdot \sigma + \beta \cdot \|Vel\| \cdot \Delta t$` | `α=0.4`, `β=0.6`, `Δt=0.3` с |
| **Вход Long** | `D_exp ≥ D_min_net` ∧ `\|Z\| ≥ Z_threshold` ∧ `Vel > velocity_min` ∧ `EMA_50 > EMA_200` ∧ **lag filters §3.5** | см. regime matrix |
| **Вход Short** | `D_exp ≥ D_min_net` ∧ `\|Z\| ≥ Z_threshold` ∧ `Vel < −velocity_min` ∧ `EMA_50 < EMA_200` ∧ **lag filters §3.5** | см. regime matrix |
| **Lag open (обяз.)** | `lag_residual ≥ lag_min_bps` | Bybit **ещё не** догнал импульс Binance |
| **Follow-through gate** | `follow_through_rate_1h ≥ follow_through_min` **или** `vol_regime = high` | Rolling §3.5; low FT → halt entries |
| **Impulse confirm** | `\|impulse_bps_100ms\| ≥ impulse_min_bps` ∧ volume USD ≥ `volume_threshold_usd` | Не один тик |
| **Net edge est.** | `D_exp ≥ D_min_net` **и** `lag_residual × capture_est ≥ D_min_net` | Fee-aware §6.3 |
| **Regime matrix** | | |
| — Range (0) | Long ✓, Short ✓ | |
| — Transition (1) | Long ✓, Short ✓ | |
| — Trend (2) | Long только если `EMA_50 > EMA_200` | Short только если `EMA_50 < EMA_200` |
| **Стоп-лосс (initial)** | `$SL = P_{entry} \pm (ATR \times K)$` | `K = 1.8` |
| **TP Fixed** | `$TP = P_{entry} \times (1 \pm TP_{init})$` | `TP_init = 0.005`; partial 50% |
| **TP Trail** | `$TP = P_{curr} \mp ATR \times K_{tp}$` | monotonic; §5.0 |
| **K_tp adaptive** | `$K_{tp} = clamp(K_{base} \times (1 + 0.1 \times \|Z\|), 0.8, 1.5)$` | `K_base = 1.0` |
| **EMA** | `$EMA_t = (P_t - EMA_{t-1}) \times \frac{2}{n+1} + EMA_{t-1}$` | `n ∈ {50, 200, 500}` |
| **Welford** | `σ = sqrt(M2 / (n-1))` | окно 1000 тиков |
| **Regime** | `TrendStrength = \|EMA_50 - EMA_200\| / ATR` | Range < 1.2; Trend > 2.0 |
| **Exhaustion Exit** | `\|Vel\| < 0.00005` ∧ `\|Z\| < 0.5` | Vel из пакета (Binance) |
| **Спред-фильтр (Observer)** | `spread_pct > 0.0001` | pause signal |
| **Волатильность-фильтр** | `σ/price < 0.0005` ∨ `ATR/price < 0.002` | `entry_valid = 0` |

**Результат Entry Engine → пакет:**
- Условия Long → `entry_valid=1`, `direction_bias=+1`
- Условия Short → `entry_valid=1`, `direction_bias=−1`
- Иначе → `entry_valid=0`, `direction_bias=0`

---

## 8. Мониторинг, логирование, безопасность и алерты

### 8.1. Логирование
- Библиотека: `tracing` + `tracing-subscriber` (async file appender).
- Формат `.bin` v1:
  ```
  [8 bytes] timestamp_ns (u64, UTC)
  [2 bytes] event_type (u16): 1=TICK, 2=PACKET, 3=ORDER, 4=FILL, 5=EXIT
  [4 bytes] payload_len (u32)
  [N bytes] payload (postcard)
  ```
- Ротация: 100 МБ или 1 час. Сжатие `zstd` в фоне.

### 8.2. Метрики (Prometheus)
- `latency_p95_ms`, `latency_p99_ms`, `packet_loss_count`, `seq_gap_count`, `pause_entries_active`
- `slippage_avg_pct`, `positions_open_count{type="spot"|"futures"}`
- `tp_hit_count{type="partial"|"full"}`, `tp_trail_distance_pct`, `sl_phase_count`
- `daily_drawdown_pct`, `heartbeat_status{node="tokyo"|"singapore"}`
- `follow_through_rate_1h{symbol}`, `net_edge_bps_rolling{symbol}`, `lag_residual_bps{symbol}`
- `process_resident_memory_bytes`, `cpu_usage_percent`, `zenoh_publish_p99_us`
- `risk_engine_hot_path_us`, `risk_flags_stale_ms`
- Endpoint: `/metrics`, scrape interval: 5s.

### 8.3. Каналы алертов
| Канал | Назначение | SLA | Реализация |
|-------|------------|-----|------------|
| **Telegram** | Оператор: вход/выход, статус, ошибки | < 10 сек | `teloxide` на Executor |
| **Prometheus Alertmanager** | DD > 1.5%, P95 latency > 150ms, gap storm, API disconnect, watchdog timeout | Мгновенно | `stop_all` webhook |
| **Email** | Ежедневный аудит PnL, win-rate, slippage | < 5 мин | SMTP, 00:00 UTC |

### 8.4. Безопасность
- API Keys: AWS KMS или Hashicorp Vault. Дешифровка при старте в RAM.
- IP Whitelist на Binance/Bybit.
- Permissions: trade + read, без вывода. Spot Margin — отдельное разрешение.
- Ротация ключей: раз в 30 дней через CI/CD.
- Конфиг: `.toml` не коммитится; AWS Parameter Store / env.

### 8.5. Панель управления (Control Panel)

Отдельный сервис **`control-panel`** на Executor (Сингапур) или выделенном admin-хосте в той же VPC. Назначение — операторское управление капиталом, парами и мониторинг профита **без правки конфигов вручную на сервере**.

#### 8.5.1. Общие требования

| Параметр | Спецификация |
|----------|--------------|
| **Доступ** | HTTPS (TLS), Basic Auth или JWT; bind только на private IP / VPN / SSH-tunnel. Публичный интернет без mTLS **запрещён**. |
| **Обновление UI** | WebSocket push каждые 1–2 с (позиции, PnL, статус пар); REST — для мутаций. |
| **Hot path** | Панель **не участвует** в hot path Risk Engine и исполнении ордеров. Все команды — async через command queue Executor. |
| **Аудит** | Каждое изменение (allocation, enable/disable pair) → `tracing` event + append-only audit log. |

#### 8.5.2. Распределение капитала по парам (% от депозита Spot / Futures)

**Spot и Futures — разные кошельки (§6.0).** Allocation задаётся **отдельно** для каждого типа счёта.

| Поле | Применимо к | Смысл |
|------|-------------|-------|
| `spot_alloc_pct` | `instrument = "spot"` | % от **spot equity** (USDT на спот-счёте) |
| `futures_alloc_pct` | `instrument = "futures"` | % от **futures equity** (маржа USDT Perpetual) |

- Единица: доля (`0.10` = 10%). Сумма `spot_alloc_pct` по всем enabled spot-парам ≤ `1.0`. Сумма `futures_alloc_pct` по enabled futures-парам ≤ `1.0`. Остаток — резерв на соответствующем кошельке.
- Одна пара = один instrument (spot **или** futures), не оба одновременно.
- Изменение allocation:
  - **Без открытой позиции** — немедленно; warm path ≤ 500 ms.
  - **С открытой позицией** — только для следующего входа.
- Размер позиции: `Qty = WalletEquity × AllocPct × Risk% / (ATR × Multiplier)` (§6.1).
- Валидация: `0 < alloc_pct ≤ 0.25` на пару в рамках своего кошелька.

**Dashboard в UI — два блока:**

```
┌─ SPOT ─────────────────────────────┐  ┌─ FUTURES ──────────────────────────┐
│ Equity: 300 USDT                     │  │ Equity: 200 USDT                   │
│ Allocated: 65%  Free: 35%            │  │ Allocated: 80%  Free: 20%          │
│ Net PnL today: +2.1 USDT (+0.7%)     │  │ Net PnL today: +4.3 USDT (+2.1%)   │
│ [Stop new entries] [Cancel all orders]│  │ [Stop new entries] [Cancel all orders]│
└──────────────────────────────────────┘  └────────────────────────────────────┘
```

**Таблица пар (фрагмент):**

| Пара | Type | Enabled | Alloc % | Wallet | Net PnL | Pos |
|------|------|---------|---------|--------|---------|-----|
| BTCUSDT | Futures | ✓ | 20% | Futures 200 | +1.2% | Long |
| ETHUSDT | Spot | ✓ | 15% | Spot 300 | +0.4% | — |
| SOLUSDT | Futures | ⏸ | 10% | Futures 200 | — | — |

#### 8.5.3. Добавление и остановка пар

| Действие | Поведение |
|----------|-----------|
| **Add pair** | Символ + `instrument` (spot/futures) + `spot_alloc_pct` или `futures_alloc_pct`. Observer: `SubscribeSymbol`. SLA ≤ 30 с. |
| **Stop pair (disable)** | `enabled = false` для пары. **Новые входы** блокируются (`PAUSE_OK`-аналог на уровне пары). Открытая позиция **не закрывается** автоматически — Position Manager продолжает SL/TP. Observer: `UnsubscribeSymbol` после подтверждения «нет позиции» или по явной галочке «force stop + close» (эквивалент `/flush` только для этой пары). |
| **Remove pair** | Только если `enabled = false` и нет позиции. Запись удаляется из runtime-конфига; Observer отписывается. |
| **Лимит whitelist** | 20–35 пар; при попытке добавить сверх лимита — ошибка UI. |

> **Согласование с §3.1:** автоматическая подписка/отписка без оператора по-прежнему запрещена; панель — единственный легитимный канал изменения состава пар.

#### 8.5.4. Отображение профита (Profit)

Панель обязана показывать:

| Метрика | Описание | Обновление |
|---------|----------|------------|
| **Unrealized PnL (net)** | После estimated fees | WebSocket 1–2 с |
| **Realized PnL (session, net)** | Закрытые сделки минус fees | По fill |
| **Realized PnL (today / 24h, net)** | Календарные сутки UTC | REST + WS |
| **Realized PnL (total, net)** | С `initial_spot_deposit` + `initial_futures_deposit` (§10.1) | REST |
| **Gross vs Net** | Обе колонки; **Net — primary** (§6.3) | REST |
| **Profit by pair** | realized + unrealized net, win-rate, fees, slippage | REST |
| **Equity curve** | Spot и Futures **отдельные** графики | REST |
| **Fees paid** | Spot fees / Futures fees раздельно | REST |

Формат отображения: USDT (абсолют) и **% от депозита** (§1.3, `pnl_pct`). Цветовая индикация: profit ≥ 0 — зелёный; DD > порога §6.2 — красный баннер.

#### 8.5.5. REST API (минимальный контракт)

```
GET  /api/v1/dashboard                    → spot/futures equity, alloc, net_pnl, pairs[]
GET  /api/v1/pairs                        → enabled, spot_alloc_pct | futures_alloc_pct
POST /api/v1/pairs                          → { symbol, instrument, alloc_pct, leverage? }
PATCH /api/v1/pairs/{id}                  → { enabled?, spot_alloc_pct?, futures_alloc_pct? }
DELETE /api/v1/pairs/{id}
GET  /api/v1/profit?period=24h&wallet=spot|futures|all
GET  /api/v1/positions?wallet=spot|futures|all
POST /api/v1/positions/{id}/close
POST /api/v1/trading/halt                 → { wallet: "spot"|"futures"|"all", halt_entries: true }
POST /api/v1/trading/resume               → { wallet: "spot"|"futures"|"all" }
POST /api/v1/orders/cancel-all            → { wallet: "spot"|"futures" }  # снять все открытые ордера
POST /api/v1/positions/close-all          → { wallet: "spot"|"futures", confirm: true }
GET  /api/v1/suggestions?status=pending   → pending Analyst proposals (Фаза 2)
POST /api/v1/suggestions/{id}/apply       → one-click apply
POST /api/v1/suggestions/{id}/reject
WS   /ws/v1/stream                        → + push «N pending suggestions»
```

#### 8.5.6. Конфигурация

```toml
[control_panel]
enabled = true
bind_addr = "127.0.0.1:8080"       # только private; nginx reverse proxy опционально
auth_mode = "jwt"                   # "basic" | "jwt"
jwt_secret_env = "PANEL_JWT_SECRET"
default_spot_alloc_pct = 0.05
default_futures_alloc_pct = 0.05
max_pair_alloc_pct = 0.25
ws_push_interval_ms = 2000
audit_log_path = "/var/log/bot/panel_audit.jsonl"
```

Расширение `symbols.toml`:

```toml
[[symbol]]
id = 2
binance = "ETHUSDT"
bybit = "ETHUSDT"
instrument = "spot"
spot_margin_enabled = false
enabled = true
spot_alloc_pct = 0.15               # только для instrument = "spot"
# futures_alloc_pct = 0.20          # только для instrument = "futures"
```

#### 8.5.7. Метрики Prometheus (дополнение к §8.2)

- `panel_pair_enabled{symbol}` — 0/1
- `panel_spot_alloc_pct{symbol}`, `panel_futures_alloc_pct{symbol}`
- `profit_net_usdt{wallet="spot"|"futures"}`
- `halt_entries_active{wallet="spot"|"futures"}`

#### 8.5.8. Связь с Telegram (§10.3)

Telegram — мобильные алерты; панель — основной UI. `/status` — краткая сводка Spot/Futures отдельно.

#### 8.5.9. Остановка торговли и снятие ордеров (Spot / Futures раздельно)

Оператор должен иметь **независимый контроль** над Spot и Futures.

| Команда (панель / API) | Spot | Futures | Эффект |
|------------------------|------|---------|--------|
| **Stop new entries** | `halt_entries_spot = true` | `halt_entries_futures = true` | Блок новых входов; **открытые позиции** продолжают SL/TP (§5.4) |
| **Resume entries** | `halt_entries_spot = false` | `halt_entries_futures = false` | Снятие блокировки входов |
| **Cancel all orders** | `POST .../orders/cancel-all?wallet=spot` | `...wallet=futures` | Отмена **всех** открытых ордеров на счёте (limit, stop, conditional). Позиции **не закрываются** |
| **Close all positions** (опционально, confirm) | spot only | futures only | Market close всех позиций + cancel orders |
| **Stop ALL** | оба флага halt | | + cancel all orders на обоих счетах |

**Cancel all orders — детали:**
- Spot: `POST /v5/order/cancel-all` (`category=spot`).
- Futures: `POST /v5/order/cancel-all` (`category=linear`).
- Включает: pending limits, conditional. **Exchange stop-loss (§4.5) тоже снимается.**
- **Обязательно после cancel:** если есть открытая spot-позиция → auto **re-place exchange stop** по `effective_SL` (§5.5) в течение 2 с; UI-предупреждение + Telegram «stop restored».
- Futures: после cancel re-place `Stop-Market` по `effective_SL` (§5.1).
- **Не путать** с «stop pair» (§8.5.3): halt блокирует только **новые** входы.

**UI:** две кнопки на каждый кошелёк — «⏸ Stop new bets» и «✕ Cancel all orders»; статус `HALTED` / `ACTIVE` в dashboard.

### 8.6. Analyst Service — ИИ-аналитик (offline-советник, **Фаза 2**, не источник edge)

Сервис **`analyst`** — автономный процесс в **Фазе 2** (§10.6). **Не создаёт alpha** и **не заменяет** Observer (§1.8). Задача — **увеличить net PnL через фильтрацию режима**: когда бот ON/OFF, alloc между 2–3 парами, tuning порогов, редкие `manual_entry` через Apply.

| Analyst делает | Analyst **не** делает |
|----------------|----------------------|
| «Сегодня флэт — halt entries 2 h» | Вход на каждый тик Binance |
| «Follow-through на ETH упал — disable pair» | LLM в hot path ≤150 ms |
| «Поднять z_score_entry до 2.8» (Apply) | Auto-apply без оператора |
| Daily digest + MA divergence alert | Гарантия 2%/день |

**Исполнение — только после Apply оператором** (§8.6.4), в т.ч. с телефона через Telegram.

> **Analyst не автоторгует.** Нет Apply — нет изменений. Hot path бота Analyst **не трогает**. Эффект Analyst: **PF с фильтром > PF без** на replay (§9.0).

#### 8.6.1. Архитектура (Фаза 2)

```
[ Trading Bot — Фаза 1 ]          [ Analyst — Фаза 2 ]           [ Operator (mobile/PC) ]
 Observer + Executor + Panel            │                              │
      │                                  │                              │
      ├── .bin + trades DB ─────────────►│ MA + orderbook patterns      │
      ├── Panel command queue ◄──────────│ SuggestionQueue              │
      │                                  │ LLM forecast                 ├──► Telegram 🔔
      └── POST /suggestions/{id}/apply ◄─┤ Proposal builder             │    [Apply][Reject]
                                         └── TimescaleDB §8.7           └──► Panel «Pending»
```

| Параметр | Спецификация |
|----------|--------------|
| **Deploy** | `analyst.service`; **стартует после** ≥ 14 дней сбора данных Фазы 1 |
| **Runtime** | Python 3.11+ (рекомендуется) |
| **Расписание** | Forecast каждые 15 min; daily report 00:30 UTC; scan pending → push оператору |
| **LLM** | OpenAI / Anthropic / Ollama; timeout 60 s → rule-based fallback |

#### 8.6.2. Наблюдение за MA (Moving Averages)

Analyst строит и отслеживает MA **независимо** от торгового бота:

| MA | Источник | Назначение |
|----|----------|------------|
| **EMA(50), EMA(200), EMA(500)** Binance | `.bin` PACKET; EMA(500) — из TICK или REST klines | Тренд сигнального рынка (§3.3) |
| **EMA(50), EMA(200)** Bybit | `.bin` + пересчёт из Bybit mid | Тренд рынка исполнения |
| **SMA(20), SMA(50)** (опц.) | REST klines 5m Binance + Bybit | Краткосрочный контекст |
| **Cross state** | `EMA50 vs EMA200` на обеих биржах | Golden/Death cross; расхождение Binance↔Bybit |

**Rule-based MA (детерминированно, до LLM):**
```
ma_bias = +1  если EMA50 > EMA200  и  price > EMA50   (uptrend)
ma_bias = −1  если EMA50 < EMA200  и  price < EMA50   (downtrend)
ma_bias =  0  иначе                                    (range/chop)

ma_spread_pct = (EMA50 − EMA200) / price
ma_divergence = sign(ma_bias_binance) ≠ sign(ma_bias_bybit)  → alert
```

#### 8.6.3. Прогноз направления (LLM + MA)

Прогноз на горizont `forecast_horizon_min` (default 30 min) — **не ордер**, а оценка:

**Вход LLM (JSON):** symbol, ts, binance `{price, ema50, ema200, z, vel, regime}`, bybit `{mid, ema50, ema200}`, `ma_bias_*`, `bot_last_signal`, `recent_trades_net_pnl_pct`.

**Выход (обязательная схема):**
```json
{
  "direction_forecast": "up" | "down" | "neutral",
  "confidence": 0.0,
  "horizon_min": 30,
  "ma_summary": "EMA50>EMA200 на обеих биржах",
  "divergence_vs_bot": "aligned" | "contradicts" | "no_bot_signal",
  "risk_note": "",
  "suggestions": []
}
```

**Constraints:** `confidence < 0.55` → `neutral`; forecast ≠ bot → alert.

#### 8.6.4. Proposal & Apply — one-click применение (оператор не at desk)

Analyst создаёт **формализованные предложения** (`Suggestion`), оператор **одной кнопкой** отправляет их в Panel → Executor command queue.

**Типы предложений (`SuggestionKind`):**

| Kind | Что предлагает | После Apply |
|------|----------------|-------------|
| `config_patch` | `{ "signals.z_score_entry": 2.8 }` | hot-reload config |
| `pair_alloc` | `{ symbol, futures_alloc_pct: 0.12 }` | §8.5.2 |
| `pair_enable` / `pair_disable` | вкл/выкл пару | §8.5.3 |
| `manual_entry` | `{ symbol, direction, instrument, size_pct }` | Risk Engine → open (§4.2) |
| `close_position` | `{ position_id, reason }` | Market close |
| `halt_wallet` | `{ wallet: "spot"\|"futures", halt: true }` | §8.5.9 |
| `trading_window` | `{ symbol, enabled_hours_utc: [13,14,...] }` | hot-reload из edge_profile §9.0 |

**Жизненный цикл:**
```
Analyst → POST /internal/suggestions (status=pending)
       → Push: Telegram + Panel badge «N pending»
Operator → [Apply] или [Reject]
       → Panel POST /api/v1/suggestions/{id}/apply
       → Executor validates + executes (≤2 s)
       → status=applied|rejected|expired; audit log
```

**Telegram (teloxide / Analyst bot):**
```
📊 Analyst · BTCUSDT
Прогноз: UP (conf 72%)
Предложение: manual_entry Long futures, 5% alloc
Обоснование: EMA50>200, Z=2.3, стакан bid-heavy
[✅ Apply] [❌ Reject] [📋 Details]
```
- Callback `apply:{suggestion_id}` / `reject:{suggestion_id}`
- TTL предложения: **24 h** (config `suggestion_ttl_hours`); expired → auto-reject
- **Rate limit Apply:** max 10 applies / час / оператор

**Безопасность Apply:**
- `manual_entry` проходит **полный Risk Engine**; отклоняется если DD/halt/fees fail
- `manual_entry` max size: `min(proposed_size_pct, max_manual_entry_pct)` default **5%** alloc
- `config_patch` — только whitelist ключей (`signals.*`, `take_profit.*`, `risk.*` — не API keys)
- Replay-check (опц.): если `require_replay_on_config = true`, Apply config только если replay PF не падает > 10%

**Panel API (дополнение §8.5.5):**
```
GET  /api/v1/suggestions?status=pending
GET  /api/v1/suggestions/{id}
POST /api/v1/suggestions/{id}/apply      # operator JWT
POST /api/v1/suggestions/{id}/reject
```

**Структура `Suggestion`:**
```json
{
  "id": "uuid",
  "created_at": "ISO8601",
  "expires_at": "ISO8601",
  "status": "pending|applied|rejected|expired",
  "kind": "manual_entry",
  "payload": { "symbol": "BTCUSDT", "direction": "long", "instrument": "futures", "size_pct": 0.05 },
  "rationale": "LLM + MA + orderbook imbalance 0.62",
  "confidence": 0.72,
  "source": "analyst"
}
```

#### 8.6.5. Отчёты и Analyst API

| Канал | Содержимое |
|-------|------------|
| Telegram | Daily digest + **push pending suggestions** |
| Panel | Вкладки «Analyst», «Pending (N)» |
| Trade journal | → TimescaleDB (§8.7) |

```
GET  /analyst/v1/forecasts
GET  /analyst/v1/forecasts/{symbol}
GET  /analyst/v1/reports/daily?date=
POST /analyst/v1/analyze-now
POST /analyst/v1/suggestions/generate    # internal / cron
```

#### 8.6.6. Trade journal → БД

Поля: `entry_ts`, `exit_ts`, `symbol`, `instrument`, `direction`, prices, `z_entry`, `vel_entry`, `d_exp`, `d_min_net`, `regime`, `ma_bias_*`, `ob_imbalance_entry` (§8.7), `slippage_pct`, `fees_usdt`, `net_pnl_pct`, `exit_reason`. Export из Executor 1×/час → PostgreSQL/TimescaleDB.

#### 8.6.7. Конфиг `analyst.toml`

```toml
[analyst]
enabled = false                       # true только в Фазе 2
min_trading_days_before_start = 14    # ждём накопления БД
llm_provider = "openai"
llm_model = "gpt-4o-mini"
forecast_interval_min = 15
suggestion_ttl_hours = 24
max_manual_entry_pct = 0.05
push_pending_to_telegram = true
database_url = "postgres://bot@localhost/bot_warehouse"

[analyst.ma]
ema_periods = [50, 200, 500]

[phase3]                               # §10.7 — disabled до pass gates
enabled = false
auto_apply_level = "off"
shadow_mode = true
shadow_min_days = 30
require_replay_on_config = true

[phase3.path_b]
enabled = false
experiment_deposit_usdt = 150.0
shared_observer_fanout = true
```

#### 8.6.8. Go/No-Go перед live (Фаза 1 — без Analyst)

| # | Критерий | Порог | Блокирует live? |
|---|----------|-------|-----------------|
| 0 | **Edge Research** §9.0 завершён | `net_edge_bps > 0` на top-quartile сигналах, ≥7 дней данных | **Да** |
| 1 | Profit Factor (replay + paper) | ≥ **1.2** | **Да** |
| 2 | Follow-through @ 150 ms delay | ≥ **40%** (Bybit в сторону `direction_bias` за 300 ms) | **Да** |
| 3 | Max drawdown paper | < **10%** futures equity | **Да** |
| 4 | Paper trades futures | ≥ **100** сделок | **Да** |
| 5 | Analyst forecast accuracy | ≥ 55% | **Нет** (Фаза 2) |
| 6 | Rolling 30d expectancy | **> 0** net после fees (paper или live staged) | **Да** |
| 7 | Live staged | 1% депозита, **7 дней** без critical errors | **Да** |

#### 8.6.9. Связь с ботом

- Без **Apply** Analyst **не влияет** на торговлю (Фазы 0–2).
- Apply → Panel command queue → тот же путь, что ручные действия оператора (§8.5).
- **Auto-apply запрещён** до pass §10.7.2; graduated levels — §10.7.4.

### 8.7. Order Book Warehouse — накопление стаканов и паттернов (**Фаза 2**)

Параллельно с Analyst — сбор и хранение данных стакана для offline-анализа и обогащения LLM.

#### 8.7.1. Источники и частота

| Биржа | Поток | Частота | Глубина |
|-------|-------|---------|---------|
| Binance Futures | `@depth10@100ms` | 10 Hz | 10 levels |
| Bybit Linear | `@orderbook.50` или REST snapshot | 10–20 Hz | 50 levels |
| Bybit Spot | `@orderbook.50` | 10 Hz | 50 levels |

**Collector `book-collector`** (Rust или Python sidecar, **не hot path**):
- Подписка на те же пары, что whitelist
- Snapshot + delta → нормализованный формат → batch insert в БД каждые 1–5 с

#### 8.7.2. Схема БД (TimescaleDB / PostgreSQL)

```sql
-- orderbook_snapshots (hypertable по ts)
ts TIMESTAMPTZ, symbol TEXT, exchange TEXT,  -- binance_futures | bybit_linear | bybit_spot
bid_prices DOUBLE PRECISION[], bid_qtys DOUBLE PRECISION[],
ask_prices DOUBLE PRECISION[], ask_qtys DOUBLE PRECISION[],
mid DOUBLE PRECISION, spread_pct DOUBLE PRECISION,
imbalance DOUBLE PRECISION,              -- (bid_vol - ask_vol) / (bid_vol + ask_vol) top-10
depth_bid_usd DOUBLE PRECISION, depth_ask_usd DOUBLE PRECISION

-- orderbook_features (агрегаты 1 min — для Analyst)
ts, symbol, exchange,
imbalance_mean, imbalance_std, spread_mean,
bid_wall_detected BOOL, ask_wall_detected BOOL,
binance_bybit_imbalance_delta DOUBLE PRECISION
```

Retention: raw snapshots **30 дней**; минутные features **1 год**. Сжатие TimescaleDB continuous aggregates.

#### 8.7.3. Паттерны для Analyst

| Паттерн | Описание | Использование |
|---------|----------|---------------|
| **Imbalance momentum** | Δimbalance за 30 s | Подтверждение direction forecast |
| **Wall appearance** | bid/ask wall > 3× median size | `risk_note` в suggestion |
| **Binance→Bybit lag** | imbalance Binance опережает Bybit | Оценка follow-through |
| **Spread widening** | spread > p95 | Предложение halt или skip pair |
| **Pre-signal footprint** | ob shape за 60 s до `entry_valid` | Offline tuning Z/Vel |

Analyst при генерации `manual_entry` / forecast **обязан** прикладывать `ob_imbalance`, `wall_detected` из `orderbook_features`.

#### 8.7.4. Минимальный объём перед включением Analyst

| Метрика | Порог |
|---------|-------|
| Календарных дней сбора | ≥ 14 |
| Snapshots на пару | ≥ 100 000 |
| Закрытых сделок в journal | ≥ 50 |

---

## 9. Тестирование, валидация и развертывание

### 9.0. Edge Research — доказательство lag **до** paper/live (обязательно)

> **Без §9.0 live запрещён.** Цель — ответить: «есть ли деньги на наших 2–3 парах при delay 150 ms?»

**Срок:** 1–2 недели. **Инструменты:** Observer lag telemetry (§3.5) **или** standalone Python collector; Parquet; pandas/jupyter (offline).

**Сбор (каждые 100 ms per symbol):**

| Поле | Назначение |
|------|------------|
| `binance_mid`, `bybit_mid` | Lag |
| `impulse_bps_100ms` | Триггер события |
| `fwd_ret_bybit_200ms`, `_500ms`, `_1000ms` | Follow-through |
| `hour_utc`, `atr_pct` | Heatmap режимов |

**Анализ (артефакты в repo `research/edge_report/`):**

1. **Follow-through rate** по `(symbol, hour_utc, vol_bucket)`.
2. **Conditional return** Bybit после `impulse ≥ impulse_min_bps`.
3. **Net edge bps** = conditional return − fee_round_trip − slippage_assumption (0.05%).
4. **Top-quartile windows** — часы/vol, где `net_edge_bps > 0`.

**Выход §9.0 → `edge_profile.toml`:**

```toml
[edge.BTCUSDT]
follow_through_min = 0.42          # из данных, не guess
lag_min_bps = 3.0
trade_hours_utc = [13, 14, 15, 16, 17, 18, 19, 20]  # пример
vol_regime_min_atr_pct = 0.0025
```

**Go §9.0:** хотя бы **1 пара** с `net_edge_bps > 0` в ≥3 часовых окнах; иначе **stop** — стратегия не monetizable на текущем депозите/infra.

### 9.1. Тестирование
| Этап | Инструменты | Критерии приемки |
|------|-------------|------------------|
| **Edge Research** | §9.0 collector + report | `net_edge_bps > 0`; `edge_profile.toml` generated |
| **Resource budget** | §2.7 limits on t3.micro | RAM/CPU within soft limits 24 h paper |
| **Lag gates** | Unit §7 + §3.5 | No entry when `lag_residual < lag_min_bps`; convergence exit fires |
| Unit/Integration | `cargo test`, `mockall`, `proptest` | Coverage ≥ 80% критического пути; 0 критических багов |
| Entry/Risk split | Mock packets | Executor **не** вызывает Z/D_exp; reject при `entry_valid=0` |
| SL/TP state machine | proptest порогов | Monotonic SL/TP; порядок 0.15→0.30→TP-0 |
| Network Simulation | `tc netem` (delay 50ms jitter 5ms loss 0.1%) | Safe-Mode; gap storm pause; ордера не дублируются |
| Replay Engine | Симулятор на `.bin` логах | TCA: slippage, fees, latency; PF ≥ 1.2 для live |
| **Latency replay** | Inject `injected_latency_ms = 150` | **Follow-through rate ≥ 40%**: доля сигналов, где Bybit движется в сторону `direction_bias` в окне 300 ms после delayed entry |
| Paper Trading | Bybit Testnet + Binance Futures Testnet | ≥ 100 сделок **Futures**; Spot — только после futures PF ≥ 1.3; DD < 1% |
| Live Staged | 1% депозита | 7 дней без критических ошибок |
| **Control Panel** | API + UI e2e | Spot/Futures alloc раздельно; halt entries spot ≠ futures; cancel-all orders; net PnL после fees |
| **Fee edge** | Unit + replay | Сделки с `D_exp < D_min_net` не открываются; BE включает fees |
| **Binance-adaptive SL/TP** | Replay + proptest | `effective_SL` монотонен; ≥ fee-BE; §5.5 |
| **Safe-Mode phased** | Integration | Фаза 1 не закрывает; фаза 3 закрывает все |
| **Cancel + restore stop** | Integration | После cancel-all exchange stop восстановлен ≤ 2 s |
| **MVP mono-node** | §2.6 deploy | **2–3** futures pairs; spot disabled; PF paper ≥ 1.2 |
| **Analyst Service** | §8.6.4 | Proposal+Apply e2e; manual_entry через Risk; TTL expire |
| **Order Book DB** | §8.7 | ≥100k snapshots/пара; features 1 min |
| **Go/No-Go** | §8.6.8 checklist | Пункты 0–4, 6–7 pass перед live |

### 9.2. CI/CD и развертывание

**Фаза 1 (минимальный deploy):**
- `bot-mvp.service` (или `observer` + `executor`), `telegram-alerts.service`, `control-panel.service`

**Фаза 2 (добавляется к Фазе 1):**
- `book-collector.service`, `analyst.service`, PostgreSQL/TimescaleDB

- Pipeline: GitHub Actions → fmt → clippy → test → release → S3 → SSH deploy.
- Конфиг: `/etc/bot/config.toml`, `symbols.toml`, `analyst.toml` (Фаза 2).
- Rollback: документирован в Runbook.

---

## 10. Приложения

### 10.1. `config.toml` (единый, без дубликатов секций)
```toml
[capital]
initial_futures_deposit_usdt = 300.0   # старт: весь депозит на futures
initial_spot_deposit_usdt = 0.0        # spot off до PF gate
risk_per_trade_pct = 0.01

[deployment]
mode = "start"                         # "start" | "scale" | "production"
instance_type = "t3.micro"
spot_enabled = false
min_futures_pf_for_spot = 1.3
start_futures_pairs = ["BTCUSDT", "ETHUSDT"]   # + опц. SOLUSDT
max_symbols = 35                       # SymbolRegistry §3.4
edge_profile_path = "edge_profile.toml"  # output §9.0 Edge Research

[resources]                            # §2.7 — budget t3.micro
depth_enabled = false                  # true только на scale / RAM headroom
binance_ws_connections = 1             # 1–2 на старт
zenoh_publish_hz_cap = 100
observer_ram_soft_limit_mib = 400
executor_ram_soft_limit_mib = 350
log_tick_debug = false                 # true только staging ≤1h
tokio_worker_threads = 2               # observer/executor на micro

[lag]                                  # §3.5, §7 — пороги; fine-tune из edge_profile
impulse_min_bps = 5.0
lag_min_bps = 3.0
follow_through_min = 0.40              # rolling 1h; override per-symbol in edge_profile
convergence_exit_ratio = 0.75          # TP: Bybit догнал 75% импульса
time_stop_ms = 8000
capture_est = 0.6                      # для net_edge est. в Entry Engine

[fees]
spot_maker_pct = 0.001
spot_taker_pct = 0.001
futures_maker_pct = 0.0002
futures_taker_pct = 0.00055
fee_profit_buffer_pct = 0.0003      # мин. edge поверх round-trip fees

[safe_mode]
heartbeat_miss_caution = 1            # фаза 1: halt entries
heartbeat_miss_defensive = 3          # фаза 2: SL → fee-BE
heartbeat_miss_emergency = 5          # фаза 3: close all
heartbeat_emergency_timeout_ms = 500

[spot]
spot_min_tp_pct = 0.008               # min TP для spot (0.8%); выше futures default

[execution]
default_leverage_futures = 10
max_leverage_futures = 20
margin_mode = "isolated"
slippage_limit_pct = 0.0005
use_limit_fallback = true
limit_fallback_timeout_ms = 50
limit_offset_pct = 0.0001
slippage_adaptive_resize = true

[risk]
max_daily_drawdown_spot = 0.02
max_daily_drawdown_futures = 0.015
correlation_limit_btc_eth = 0.6      # суммарно одного направления
correlation_limit_eth_when_btc_open = 0.3
atr_multiplier_stop = 1.8
atr_min_filter = 0.002

[take_profit]
enabled = true
initial_target_pct = 0.005           # TP-0: +0.5% — partial close
partial_close_pct = 0.5
trail_arm_pct = 0.003                # PnL ≥ 0.3%: TrailArm (до partial)
sl_breakeven_pct = 0.003             # PnL ≥ 0.3%: SL → fee-BE (§6.3)
sl_tighten_pct = 0.0015              # PnL ≥ 0.15%: SL-1
base_tp_trail_atr = 1.0
extended_trend_tp = true             # TP-Extended: только Futures
extended_trend_z_min = 2.0

[signals]
z_score_entry = 2.5                  # игнорируется при use_dynamic_thresholds=true
z_score_exit = 0.5
velocity_min = 0.0001                # 0.01%/s

[noise_filter]
min_trade_volume_usd = 10000.0
max_spread_pct = 0.0001
volatility_stddev_threshold = 0.0005

[network]
max_latency_ms = 150
heartbeat_interval_ms = 100
heartbeat_timeout_ms = 500
safe_mode_latency_p95_ms = 150
packet_version = 2
seq_gap_pause_threshold = 10
seq_gap_pause_duration_sec = 5

[adaptive_strategy]
use_dynamic_thresholds = true
threshold_window_ticks = 5000
threshold_percentile = 0.95
threshold_multiplier = 1.1
threshold_min = 1.8
threshold_max = 3.2

[regime_filter]
mode = "auto"  # "auto" | "range" | "trend"
trend_strength_threshold = 2.0
range_strength_threshold = 1.2

[funding_basis]
enabled = true
max_funding_rate = 0.0001
basis_threshold_pct = 0.0005
ticker_poll_interval_sec = 60

[spot_failsafe]
spot_exchange_stop = true
sl_exchange_buffer_pct = 0.001
stop_amend_debounce_ms = 1000

[replay]
log_version = "v1"
log_rotation_mb = 100
log_rotation_hours = 1
min_replay_pf_for_live = 1.2

[alerts]
telegram_bot_token = "${TELEGRAM_BOT_TOKEN}"
telegram_chat_id = "${TELEGRAM_CHAT_ID}"
prometheus_endpoint = "/metrics"

[control_panel]
enabled = true
bind_addr = "127.0.0.1:8080"
auth_mode = "jwt"
jwt_secret_env = "PANEL_JWT_SECRET"
default_spot_alloc_pct = 0.05
default_futures_alloc_pct = 0.05
max_pair_alloc_pct = 0.25
ws_push_interval_ms = 2000
audit_log_path = "/var/log/bot/panel_audit.jsonl"

[whitelist]
pairs = [
  "BTCUSDT","ETHUSDT","SOLUSDT","BNBUSDT","XRPUSDT","DOGEUSDT",
  "AVAXUSDT","LINKUSDT","ADAUSDT","POLUSDT","DOTUSDT","ARBUSDT",
  "OPUSDT","NEARUSDT","PEPEUSDT","WIFUSDT","FETUSDT","RENDERUSDT",
  "ENAUSDT","SUIUSDT","TIAUSDT","SEIUSDT","JUPUSDT","WLDUSDT","NOTUSDT"
]
```

### 10.2. `symbols.toml` (пример маппинга Spot / Futures)
```toml
[[symbol]]
id = 1
binance = "BTCUSDT"
bybit = "BTCUSDT"
instrument = "futures"
leverage = 10
enabled = true
futures_alloc_pct = 0.20

[[symbol]]
id = 2
binance = "ETHUSDT"
bybit = "ETHUSDT"
instrument = "spot"
spot_margin_enabled = false   # Long only; Short → RISK_SKIP
enabled = true
spot_alloc_pct = 0.15

[[symbol]]
id = 3
binance = "SOLUSDT"
bybit = "SOLUSDT"
instrument = "futures"
leverage = 5
enabled = true
futures_alloc_pct = 0.10

# Пример Spot Margin (опционально):
# [[symbol]]
# id = 4
# binance = "XRPUSDT"
# bybit = "XRPUSDT"
# instrument = "spot"
# spot_margin_enabled = true    # Short через Bybit Spot Margin API
```

### 10.3. Telegram-бот (Executor + Analyst push, Фаза 2)

**Trading (всегда):**
- `/status` → Spot/Futures equity, net PnL, позиции, Latency, halt-флаги
- `/pause spot` / `/pause futures` / `/pause all` → Stop new entries (§8.5.9)
- `/resume spot` / `/resume futures` / `/resume all`
- `/cancel spot` / `/cancel futures` → Cancel all orders
- `/flush spot` / `/flush futures` / `/flush all` → Close all + halt

**Analyst (Фаза 2) — оператор away from desk:**
- Push-сообщение с inline-кнопками **`[✅ Apply]` `[❌ Reject]` `[📋 Details]`** на каждое `Suggestion`
- `/pending` → список активных предложений (id, kind, symbol, TTL)
- `/apply {id}` / `/reject {id}` — текстовый fallback если кнопки недоступны
- Callback `apply:uuid` → Panel `POST /api/v1/suggestions/{id}/apply` от имени оператора

### 10.4. Структуры данных (Rust)
```rust
pub const PACKET_VERSION: u8 = 3;  // v2.0: +lag fields

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct MarketStatePacket {
    pub packet_version: u8,       // = 3
    pub ts_ns: u64,               // UTC wall-clock
    pub seq_num: u32,
    pub symbol_id: u16,
    pub entry_valid: u8,          // 0 = no entry signal, 1 = valid
    pub direction_bias: i8,       // -1 Short, 0 Neutral, +1 Long (valid if entry_valid=1)
    pub regime: u8,               // 0 Range, 1 Transition, 2 Trend
    pub z_score: f32,
    pub z_threshold_used: f32,    // actual threshold applied (dynamic or fixed)
    pub velocity: f32,            // fraction per second (§1.3)
    pub sigma: f32,               // Welford σ, for audit
    pub d_exp: f32,               // expected move, for audit
    pub d_min: f32,               // min required move, for audit
    pub ema_50: f64,
    pub ema_200: f64,
    pub atr: f32,
    pub ref_price: f64,           // Binance Futures mid — basis check
    pub bybit_mid_ref: f64,       // Bybit mid (warm merge §3.5)
    pub lag_bps: f32,
    pub lag_residual_bps: f32,    // сколько Bybit ещё не догнал impulse
    pub impulse_bps_100ms: f32,
    pub spread_pct: f32,
    pub volume_usd: f32,
    pub bid_ask_imbalance: f32,
    pub volume_delta_100ms: f32,
}
// ~80 bytes postcard; не использовать #[repr(C, packed)]

pub struct PositionState {
    pub id: String,
    pub symbol_id: u16,
    pub side: Side,
    pub instrument: InstrumentType,
    pub entry_price: f64,
    pub qty: f64,
    pub qty_remaining: f64,
    pub current_stop: f64,
    pub current_tp: f64,
    pub sl_phase: u8,             // 0=Initial, 1=Tight, 2=Breakeven
    pub tp_phase: u8,             // 0=Initial, 1=TrailArm, 2=DynamicTrail, 3=Extended
    pub partial_done: bool,
    pub pnl_pct: f64,
    pub open_time_ns: u64,
    pub entry_impulse_bps: f32,   // snapshot для lag_capture_ratio §5.2
    pub lag_capture_ratio: f32,   // 0..1, обновляется каждый пакет
    pub exchange_stop_id: Option<String>,  // Spot fail-safe §4.5
}

// Executor-local (не в пакете):
pub struct BybitExitMetrics {
    pub ema_50: f64,
    pub ema_200: f64,
    pub mid: f64,
    pub volume_delta_100ms: f32,   // MICRO_OK filter (§4.2)
}
```

### 10.5. Порядок внедрения (спринты — детализация)

> Сводная дорожная карта по фазам — **§10.6**.

| Спринт | Модуль | Фаза |
|--------|--------|------|
| **−1** | Edge Research §9.0 → `edge_profile.toml` | **0** |
| **0–7** | Rust bot → Paper → Live staged (§10.6 Фаза 1) | **1** |
| **8** | Control Panel §8.5 | **1** |
| **9a** | `book-collector` + TimescaleDB §8.7 | **2** |
| **9b** | Trade journal → DB; 14 дней накопления | **2** |
| **10** | Analyst: MA + LLM + forecasts | **2** |
| **11** | Proposal & Apply: Telegram + Panel §8.6.4 | **2** |
| **12** | Shadow tracking + counterfactual PF §10.7.3 | **3** |
| **13** | Graduated auto-apply / Path B 2-й акк §10.7 | **3** |

### 10.6. Дорожная карта разработки (Фаза 1 → Фаза 2)

```
═══════════════════════════════════════════════════════════════════
  ФАЗА 0 — EDGE RESEARCH (обязательно)   цель: доказать lag → деньги
═══════════════════════════════════════════════════════════════════
  Этап 0.1  Collector §9.0 (Python или Observer §3.5)
            └── 2–3 пары, 100 ms samples, Parquet
            └── follow-through heatmap по hour/vol

  Этап 0.2  edge_profile.toml
            └── net_edge_bps > 0 хотя бы на 1 паре
            └── trade_hours_utc, follow_through_min per symbol

  ✓ Go: §9.0 pass → разрешена разработка/paper Фазы 1
  ✗ Fail: stop или смена пар/таймфрейма — live запрещён

═══════════════════════════════════════════════════════════════════
  ФАЗА 1 — ТОРГОВЛЯ + ПАНЕЛЬ          цель: эксплуатировать edge на paper/live
═══════════════════════════════════════════════════════════════════
  Этап 1.1  Dual-node 2× t3.micro (§2.4) или mono-node отладка (§2.6)
            └── Rust: Observer+Executor, **2–3** futures pairs, **×10**

  Этап 1.2  Ядро стратегии
            └── Entry Engine §7 + lag gates §3.5
            └── Exits: convergence + time stop §5.2
            └── Risk §4.2, SL/TP §5, fee-aware §6.3
            └── .bin логи, Replay + latency 150 ms

  Этап 1.3  Control Panel §8.5
            └── equity, alloc %, halt/cancel, net PnL, pairs CRUD
            └── Telegram trading commands §10.3

  Этап 1.4  Paper → Go/No-Go §8.6.8
            └── PF ≥ 1.2, follow-through ≥ 40%, 100+ сделок
            └── Live staged 1% депозита, 7 дней

  Этап 1.5  Production scale (опционально)
            └── больше пар, spot off до PF gate

  ✓ Критерий завершения Фазы 1: live futures; Panel; operator halt/status с телефона.

═══════════════════════════════════════════════════════════════════
  ФАЗА 2 — БД + ИИ АНАЛИТИК          цель: фильтр режима + Apply (не edge)
═══════════════════════════════════════════════════════════════════
  Этап 2.1  Data Warehouse §8.7
            └── PostgreSQL/TimescaleDB
            └── book-collector: Binance depth + Bybit orderbook
            └── мин. 14 дней сбора, 100k snapshots/пара

  Этап 2.2  Trade journal в БД §8.6.6
            └── связка сделок с ob_imbalance на входе
            └── continuous aggregates (1 min features)

  Этап 2.3  Analyst Service §8.6
            └── regime ON/OFF, alloc между парами, tuning
            └── LLM forecast (30 min horizon) — **не** вход
            └── pattern rules: imbalance, walls, lag Binance→Bybit

  Этап 2.4  Proposal & Apply §8.6.4
            └── SuggestionQueue: config, alloc, manual_entry, close, halt
            └── Telegram [Apply][Reject] + Panel «Pending»
            └── manual_entry через Risk Engine; TTL 24h

  ✓ Критерий: Analyst предложения; Apply с телефона; PF с фильтром ≥ PF без (replay).

═══════════════════════════════════════════════════════════════════
  ФАЗА 3 — VALIDATED ANALYST → УПРАВЛЕНИЕ     цель: масштаб доверия к Analyst
═══════════════════════════════════════════════════════════════════
  См. детали §10.7. Кратко:
  3.1  Shadow tracking (counterfactual PF)
  3.2  Graduated auto-apply (whitelist kinds)
  3.3  Path A: интеграция в основной бот  ИЛИ  Path B: 2-й Bybit-акк (A/B)

  ✓ Go Ф3: §10.7.2 gates pass + operator sign-off

═══════════════════════════════════════════════════════════════════
  OUT OF SCOPE (Ф0–Ф2): full auto-apply; Analyst в hot path; ×50; live без §9.0
  OUT OF SCOPE (Ф3): LLM в tick path; auto manual_entry без §10.7.4 gate
═══════════════════════════════════════════════════════════════════
```

| Фаза | Срок (ориентир) | Стек | Зависит от |
|------|-----------------|------|------------|
| **0** | 1–2 недели | Python/pandas, Parquet | — |
| **1** | 8–12 недель | Rust, axum, teloxide | **§9.0 pass** |
| **2** | 6–8 недель после старта БД | Python, TimescaleDB, LLM | Фаза 1 live ≥ 14 дней |
| **3** | 4–12 недель после Ф2 gate | Analyst + 2-й deploy (опц.) | §10.7.2 gates |

**Параллельность:** `book-collector` можно запустить **в конце Фазы 1** (этап 1.4 paper) — к старту Analyst БД уже накоплена.

### 10.7. Фаза 3 — Validated Analyst: интеграция или отдельный аккаунт

> **Контекст:** Фазы 0–2 доказывают edge (Rust-бот) и дают Analyst с **ручным Apply**. Фаза 3 — когда рекомендации Analyst **статистически лучше** baseline, и оператор готов **расширить** их влияние — без передачи hot path Observer/Executor LLM.

#### 10.7.1. Цели и два пути

| Цель | Описание |
|------|----------|
| **Доказать ценность Analyst на live** | PF/expectancy **с** фильтрами Analyst > **без** на rolling 30+ дней |
| **Масштабировать без риска основного депозита** | Graduated auto-apply **или** изолированный 2-й акк |
| **Сохранить Rust hot path** | Входы по-прежнему Observer §7; Analyst — regime, alloc, halt, windows |

**Path A — интеграция в основной бот** (тот же Bybit $300+):

```
Analyst → auto-apply (whitelist kinds) → Panel audit → Executor
Основной бот = единственный trading stack
```

**Path B — отдельный Bybit-аккаунт** (A/B, рекомендуется при депозите ≤ $500):

```
Account PRIMARY ("control")     Account EXPERIMENT ("analyst-led")
  Rust bot, manual Apply           Клон infra + Analyst semi-auto
  $300 baseline                    $100–300 отдельный sub-account
  PF_baseline tracked              PF_experiment vs PF_baseline
```

| | Path A | Path B |
|---|--------|--------|
| **Риск** | Один акк — ошибка Analyst бьёт весь депозит | Основной бот **не трогается** |
| **Infra** | Без дублирования | 2-й API key; тот же SG executor **или** 2× micro |
| **Когда** | Analyst stable ≥ 60 дней, operator at desk реже | **Предпочтительно** первый шаг Ф3 |
| **Rollback** | `analyst.auto_apply = false` | Отключить experiment account |

> **Hot path не меняется в обоих путях:** LLM **не** вызывает `open_position` напрямую; только config/flags/alloc через command queue (§8.6.4).

#### 10.7.2. Go/No-Go Фазы 3 (все обязательны)

| # | Критерий | Порог |
|---|----------|-------|
| 1 | Фаза 2 завершена | §10.6 этапы 2.1–2.4 ✓ |
| 2 | Analyst suggestions applied (manual) | ≥ **30** applied за 60 дней |
| 3 | Counterfactual PF (§10.7.3 shadow) | PF_shadow ≥ **1.15** и > PF_actual |
| 4 | Forecast direction accuracy | ≥ **55%** @ 30 min horizon |
| 5 | Regime filter lift | Net PnL **с** Analyst halt/windows > **без** на replay 90d |
| 6 | Operator sign-off | Panel `POST /api/v1/phase3/enable` + JWT 2FA (опц.) |

Без pass — остаёмся на **ручном Apply** (Фаза 2).

#### 10.7.3. Этап 3.1 — Shadow mode (обязательный пролог Ф3)

Analyst генерирует suggestions **как обычно**, но параллельно:

- **Не Apply** (или Apply только operator-marked subset)
- **Counterfactual tracker** записывает: «если бы Apply сработал в T, PnL был бы X»
- Метрика: `pf_shadow`, `expectancy_shadow` vs `pf_actual`

```sql
-- shadow_outcomes (TimescaleDB)
ts, suggestion_id, kind, symbol,
would_have_pnl_pct, actual_pnl_pct, delta_pct
```

**Длительность:** мин. **30 календарных дней** live primary account.  
**Go 3.2:** `pf_shadow > pf_actual` и `expectancy_shadow > 0`.

#### 10.7.4. Этап 3.2 — Graduated auto-apply

Включение по ступеням (`analyst.toml` → `[phase3]`):

| Уровень | `auto_apply_level` | Auto-apply kinds | Запрещено auto |
|---------|-------------------|------------------|----------------|
| **0** | `off` | — | всё (Фаза 2) |
| **1** | `low_risk` | `trading_window`, `halt_wallet`, `pair_disable` | config, alloc, manual_entry |
| **2** | `tuning` | + `config_patch` (whitelist §8.6.4), `pair_alloc` | manual_entry, close |
| **3** | `full_assist` | + `close_position` (TTL 5 min operator undo) | **manual_entry** без отдельного gate |
| **4** | `manual_entry` | + `manual_entry` capped | только после **60 d** level 3 + PF lift |

**Guardrails (все уровни):**

- Rate limit: max **10 auto-applies / час** (как Apply §8.6.4)
- Kill switch: Panel «**Analyst AUTO OFF**» + Telegram `/analyst off` → мгновенно level 0
- Любой auto `config_patch` → optional `require_replay_on_config = true`
- Audit log **immutable**; rollback snapshot config каждые 6 h

#### 10.7.5. Этап 3.3 — Path A: интеграция

- Один `executor` + один Bybit API key
- `analyst.auto_apply_level` по §10.7.4
- Panel: вкладка «Analyst Auto» — level, last 20 auto-applies, undo
- **Rollback:** restore config snapshot ≤ 60 s

#### 10.7.6. Этап 3.4 — Path B: отдельный аккаунт (A/B)

**Deploy:**

| Компонент | Primary (control) | Experiment (analyst-led) |
|-----------|-------------------|---------------------------|
| Bybit | Sub-account **A** | Sub-account **B** (отдельный API key) |
| Депозит | Основной ($300+) | Меньший ($100–300) |
| Analyst auto | **off** или level 0–1 | level 2–3 (по gate) |
| Observer | Shared Tokyo **или** duplicate streams | Same packets via Zenoh fan-out |
| Executor | Singapore instance 1 | Singapore instance 2 **или** same process, 2 connectors |
| KPI | `pf_primary`, DD | `pf_experiment` vs primary |

**Правила A/B:**

- Одинаковые пары и **тот же** Rust Entry Engine (§7) — сравниваем **управление**, не разные стратегии
- Experiment получает **Analyst filters** (windows, halt, alloc); Primary — baseline без auto (или manual Apply only)
- **Promotion:** если `pf_experiment > pf_primary` **90 rolling days** и DD_experiment ≤ DD_primary → рассмотреть Path A merge или перенос alloc

**Infra budget (§2.7):** 2-й акк **не требует** 2× Tokyo — достаточно fan-out Zenoh + 2 executor processes на **одном** t3.small SG при RAM headroom.

#### 10.7.7. Конфиг `analyst.toml` — секция `[phase3]`

```toml
[phase3]
enabled = false
auto_apply_level = "off"              # off | low_risk | tuning | full_assist | manual_entry
shadow_mode = true                    # true обязательно первые 30d Ф3
shadow_min_days = 30
require_replay_on_config = true
kill_switch_file = "/var/run/bot/analyst_auto_off"

[phase3.path_b]
enabled = false
experiment_api_key_env = "BYBIT_API_KEY_B"
experiment_deposit_usdt = 150.0
shared_observer_fanout = true
```

#### 10.7.8. Критерии успеха / провала Ф3

| Исход | Условие | Действие |
|-------|---------|----------|
| **Success** | PF lift ≥ 10% vs baseline 90d; DD не хуже | Повысить level или merge Path B → A |
| **Stall** | PF lift < 5% за 60d | Остаться level 1 или manual Apply |
| **Fail** | PF_shadow < PF_actual 30d | `auto_apply_level = off`; post-mortem; Analyst retrain |

#### 10.7.9. Жёсткие запреты (все подфазы Ф3)

- LLM / Analyst **не** в Observer tick loop
- Auto-apply **без** shadow period 30d
- Auto `manual_entry` на level < 4
- Path B experiment **больше** primary deposit без operator approval
- Отключение Risk Engine (§4.2) для «Analyst urgency»

---

## 11. Исправления v1.0 → v1.1 (справочно)

| # | Проблема v1.0 | Исправление v1.1 |
|---|---------------|------------------|
| 1 | `CLOCK_MONOTONIC` в межузловых пакетах | UTC `CLOCK_REALTIME` + chrony на обоих узлах |
| 2 | Freshness check сравнивал несинхронизированные monotonic часы | `utc_now_ns() - packet.ts_ns` |
| 3 | `taskset` запускал observer и executor на одной машине | Отдельные команды per-server |
| 4 | Basis filter без цены Binance в пакете | Добавлено поле `ref_price` |
| 5 | Funding из `/funding/history` | Текущий rate из `/market/tickers` |
| 6 | Дублирующая секция `[execution]` в config.toml | Объединена в одну |
| 7 | `MarketStatePacket` — три несовместимых определения | Единая структура §10.4 |
| 8 | Postcard «обратно совместим» при добавлении полей | Явный `packet_version`; несовместимость = stop |
| 9 | Z-threshold: 2.0 в §3.3 vs 2.5 в §7 | Единый порог 2.5 (или dynamic) |
| 10 | `D_min` с Leverage для Spot | Отдельные формулы Spot/Futures |
| 11 | Long/Short не формализованы | Явные условия входа §7 |
| 12 | `MATICUSDT` устарел | Заменён на `POLUSDT` |
| 13 | Нет `seq_num` при UDP без guarantee | Добавлен dedup + метрики потерь |
| 14 | `regime` в тексте §11.2, но не в struct | Поле `regime: u8` в пакете |
| 15 | Черновые блоки «для вставки» внутри документа | Интегрированы в основные разделы |
| 16 | Coverage ≥ 95% | 80% критического пути (реалистичнее) |
| 17 | `teloxide` на обоих узлах | Только Executor / alerts service |

---

## 12. Изменения v1.1 → v1.2 (справочно)

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §5.4 Dynamic Take-Profit | 4 фазы: Initial → Trail Arm → Dynamic Trail → Extended (Trend) |
| 2 | Частичное закрытие 50% | На TP-0; остаток ведётся трейлингом |
| 3 | Связь TP ↔ SL | TP-0 → SL в безубыток; TP-1 → SL Phase 1 |
| 4 | `[take_profit]` в config.toml | Все параметры конфигурируемы, `enabled = false` для отключения |
| 5 | `PositionState` расширен | `current_tp`, `tp_phase`, `qty_remaining` |
| 6 | Формулы §7 | TP Initial, TP Trail, адаптивный `K_tp` |
| 7 | Метрики Prometheus | `tp_hit_count`, `tp_trail_distance_pct` |
| 8 | Приоритет триггеров §5.3 | Safe-Mode > SL > TP > EMA > Exhaustion |

---

## 13. Изменения v1.2 → v1.3

| # | Проблема v1.2 | Исправление v1.3 |
|---|---------------|------------------|
| 1 | Неясно, кто принимает решение о входе | §1.2: Observer Entry Engine; Executor только Risk (§4.2) |
| 2 | TP-1 (0.3%) раньше TP-0 (0.5%) | §5.0: единая машина 0.15→0.30→TP-0 partial→DynamicTrail |
| 3 | «Арбитраж» vs basis filter | §1: lead-lag/momentum, не классический арбитраж |
| 4 | Spot Short без margin | §4.3: Long-only по умолчанию; margin опционально |
| 5 | Risk Engine ≤ 10 μs нереалистичен целиком | §4.2: hot path + warm cache + RiskFlags bitmap |
| 6 | Freshness 120 ms vs P99 сети | Freshness и Safe-Mode → **150 ms** |
| 7 | `Trend-only-short` не в enum regime | §7: regime matrix 0/1/2 |
| 8 | EMA Cross exit по Binance, позиция на Bybit | §5.2: exit EMA только по Bybit mid на Executor |
| 9 | Spot без биржевого SL при crash | §4.5: optional exchange Stop-Limit + watchdog |
| 10 | Pre-signed templates некорректно | §4.4: pre-allocated buffer + HMAC at order time |
| 11 | Z-threshold dynamic vs fixed | §3.3: dynamic имеет приоритет |
| 12 | Correlation 60% vs 30% ETH | §6.2 + §4.2: единая формулировка |
| 13 | RTT Safe-Mode не определён | §2.2: P95 one-way latency 10 s window |
| 14 | Нет политики UDP gap | §2.2: gap storm → pause 5 s |
| 15 | Нет единиц Velocity/PnL | §1.3: таблица единиц |
| 16 | `packet_version = 1`, не хватало полей | `packet_version = 2`: `entry_valid`, `d_exp`, `d_min`, `sigma`, `z_threshold_used` |

---

## 14. Изменения v1.3 → v1.4

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §8.5 Панель управления | Веб-UI + REST/WebSocket API на Executor |
| 2 | `capital_alloc_pct` | Распределение капитала по парам в % от депозита |
| 3 | Enable / disable / add / remove pair | Операторское управление составом whitelist в runtime |
| 4 | Profit dashboard | Realized / unrealized PnL, по парам, equity curve, fees |
| 5 | `[control_panel]` в config.toml | Параметры сервиса панели |
| 6 | `enabled`, `capital_alloc_pct` в symbols.toml | Состояние и лимит капитала на пару |
| 7 | Спринт 8 | Внедрение Control Panel |
| 8 | Whitelist §1 | Runtime-изменение пар только через панель (не произвольная подписка) |

---

## 15. Изменения v1.4 → v1.5

| # | Добавлено / изменено | Описание |
|---|----------------------|----------|
| 1 | **Главная цель** §1 | Рост депозита; SL/TP по Binance; запрет сделок «минус комиссия» |
| 2 | §5.4 | Адаптивное поднятие SL/TP по Z, Vel, ATR, regime с Binance |
| 3 | §6.0 | **Раздельные депозиты** Spot и Futures (разные кошельки Bybit) |
| 4 | §6.3 | Fee-aware sizing: `D_min_net`, fee-BE, net PnL в панели |
| 5 | §8.5.2 | `spot_alloc_pct` / `futures_alloc_pct` — % от **своего** кошелька |
| 6 | §8.5.9 | Stop new entries + cancel all orders — **Spot и Futures раздельно** |
| 7 | `[fees]` config | Комиссии maker/taker для расчёта edge |
| 8 | Risk flags | `ENTRIES_SPOT_OK`, `ENTRIES_FUTURES_OK`, `FEE_EDGE_OK` |
| 9 | API | `/trading/halt`, `/orders/cancel-all` с `wallet=spot\|futures` |

---

## 16. Изменения v1.5 → v1.6

| # | Исправлено / добавлено | Описание |
|---|------------------------|----------|
| 1 | **D_min_net** §6.3, §7 | Убрано ошибочное `× Lev` для комиссий; fees от notional |
| 2 | §5.5 `effective_SL` | `max(sl_pnl, sl_binance, sl_fee_be)` — единый SL |
| 3 | fee-BE | Унифицирован во всех таблицах §5.0, §5.3 (убран «Entry BE») |
| 4 | `MICRO_OK` §4.2 | Фильтр по `bybit_volume_delta_100ms`, не Binance |
| 5 | §5.2.1 Safe-Mode | 3 фазы: halt → tighten SL → emergency close |
| 6 | §8.5.9 + §4.5 | cancel-all → auto re-place exchange stop ≤ 2 s |
| 7 | §2.4 MVP mono-node | Обязательный этап; spot disabled; 3–5 futures |
| 8 | §1.3, §1 | Явная цепочка Binance→Bybit; momentum (не lag-arb) |
| 9 | §6.0 Spot policy | Futures-first; spot_min_tp_pct 0.8%; PF gate 1.3 |
| 10 | §9.1 | Replay follow-through ≥ 40% @ 150 ms delay |
| 11 | `[deployment]`, `[safe_mode]`, `[spot]` config | Новые секции config.toml |
| 12 | `BybitExitMetrics.volume_delta_100ms` | Warm path microstructure |

---

## 17. Изменения v1.6 → v1.7

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §8.6 Analyst Service | Отдельный сервис: MA/EMA наблюдение, прогноз up/down/neutral |
| 2 | LLM forecast layer | JSON in/out; confidence; fallback rule-based |
| 3 | MA Binance + Bybit | EMA50/200/500; cross; divergence alert |
| 4 | Trade journal parquet | Export для Analyst и оператора |
| 5 | Go/No-Go checklist §8.6.7 | Обязательные критерии перед live |
| 6 | `analyst.toml` | Отдельный конфиг; port 8081 |
| 7 | Спринт 9 | Analyst после paper trading |
| 8 | Жёсткий запрет | Analyst не трейдер; не пишет в trading config |

---

## 18. Изменения v1.7 → v1.8

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §10.6 Дорожная карта | **Фаза 1** (бот+Panel) / **Фаза 2** (БД+Analyst) |
| 2 | §8.6.4 Proposal & Apply | SuggestionQueue; Telegram [Apply][Reject]; Panel API |
| 3 | Типы suggestions | config, alloc, manual_entry, close, halt |
| 4 | §8.7 Order Book Warehouse | book-collector, TimescaleDB, паттерны стакана |
| 5 | Operator mobile | Push pending; `/pending`, `/apply {id}` |
| 6 | Фаза 2 gate | ≥14 дней БД, 100k snapshots, 50 сделок |
| 7 | book-collector | Старт в конце Фазы 1 (paper) |
| 8 | Auto-apply | Явно **запрещён** v1.8 |

---

## 19. Изменения v1.8 → v1.9

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §1.5 SymbolRegistry | **2–3** пар старт, до 35 без рефакторинга; pre-alloc, hot-reload |
| 2 | §1.6 ROI | Реалистичные цели; **2%/день — не KPI** |
| 3 | §2.4–2.5 | Старт **2× t3.micro**; лестница scale → c7a.xlarge |
| 4 | §3.4 | Rust-скетч `SymbolRegistry` |
| 5 | `config.toml` | `$300` futures, spot off, `start_futures_pairs`, `max_symbols=35` |
| 6 | Стартовый депозит | **$300 USDT** futures-only в шапке ТЗ |

---

## 20. Изменения v1.9 → v2.0

| # | Добавлено / изменено | Описание |
|---|----------------------|----------|
| 1 | §1.7 | Экономическая модель: lag/follow-through = источник edge; формула PnL |
| 2 | §1.8 | Роли Observer (edge) vs Analyst (regime filter) vs Operator |
| 3 | Старт | **2–3 пары**, **×10**; ×50 запрещён (§6.2) |
| 4 | §3.5 | Lag telemetry, `lag_residual`, follow-through logging |
| 5 | §5.2 | Выходы **Lag Convergence**, **Time Stop**, **Invalidation** |
| 6 | §7 | Lag gates, impulse confirm, `net_edge_est` |
| 7 | §9.0 | **Edge Research** обязателен до paper/live; `edge_profile.toml` |
| 8 | §8.6 | Analyst = offline фильтр режима, не замена Observer |
| 9 | §8.6.8 | Go/No-Go: пункт 0 (Edge Research), полная таблица |
| 10 | §10.6 | **Фаза 0** Edge Research; обновлённые критерии Фаз 1–2 |
| 11 | `config.toml` | `[lag]`, `edge_profile_path`, 2 пары старт |

---

## 21. Изменения v2.0 → v2.1

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §2.7 | Принципы ресурсоёмкости: RAM/CPU budget t3.micro |
| 2 | §2.7.2 | WS limits старт: 1–2 conn, `depth_enabled = false` |
| 3 | §2.7.3 | Lean Rust workspace; **запрещённые deps** в hot-path crates |
| 4 | §3.1 | Адаптация под micro: 2 tokio threads, depth optional |
| 5 | `[resources]` | `config.toml`: RAM limits, zenoh cap, worker threads |
| 6 | §8.2 / §9.1 | Метрики и acceptance: resource budget 24 h paper |

---

## 22. Изменения v2.1 → v2.2

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §10.7 | **Фаза 3:** validated Analyst → управление торговлей |
| 2 | §10.7.1 | Path A (интеграция) vs Path B (2-й Bybit-акк, A/B) |
| 3 | §10.7.2 | Go/No-Go Ф3: shadow PF, 30 applied, operator sign-off |
| 4 | §10.7.3–3.4 | Shadow mode → graduated auto-apply (levels 0–4) |
| 5 | §10.7.5–3.6 | Deploy primary vs experiment; shared Observer fan-out |
| 6 | `[phase3]` | `analyst.toml`: auto_apply_level, path_b, kill switch |
| 7 | §10.6 | Блок Ф3 в дорожной карте; OUT OF SCOPE уточнён |
| 8 | §10.5 | Спринты 12–13 (Фаза 3) |
