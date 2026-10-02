# 002 – Tokio runtime

## Goal

Understand how Tokio executes an Axum application, how to configure the runtime, and how to use
concurrency primitives correctly inside handlers.

## Key concepts

### Why a runtime?

`async fn` returns a **future** – a state machine that does nothing until polled. Rust's standard
library has no executor; a runtime provides:

- **scheduler / executor** – polls tasks on worker threads,
- **I/O driver** – OS event notifications (epoll / kqueue / IOCP) for sockets,
- **time driver** – `sleep`, `interval`, `timeout`,
- **blocking pool** – threads for `spawn_blocking`.

Axum and hyper depend on Tokio; other runtimes (async-std, smol) are not supported.

### Runtime flavors

| Flavor           | Threads                         | Use                                     |
|------------------|---------------------------------|-----------------------------------------|
| `multi_thread`   | N workers (default = CPU cores), work-stealing | servers (default for `#[tokio::main]`) |
| `current_thread` | everything on one thread        | tests (`#[tokio::test]`), CLI tools, embedded |

```rust
#[tokio::main]                                          // multi_thread, workers = cores
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
#[tokio::main(flavor = "current_thread")]
```

Manual equivalent (used in this branch):

```rust
fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(16)
        .thread_name("booking-worker")
        .enable_all()                // I/O + time drivers
        .build()
        .unwrap()
        .block_on(run());
}
```

### Tasks

```rust
let handle: JoinHandle<T> = tokio::spawn(async move { ... });
let result: Result<T, JoinError> = handle.await;
handle.abort();                     // cancel
```

- A task is a unit of scheduling – very cheap (hundreds of bytes), millions are fine.
- Future passed to `spawn` must be `Send + 'static` (can move between threads, owns its data → `move`, `Arc`).
- Dropping a `JoinHandle` **detaches** the task, it keeps running.
- A panic inside a task is caught and returned as `JoinError` – it does not kill the process.
- `axum::serve` spawns one task per connection.

### Cooperative scheduling

Tasks yield the thread **only at `.await` points**. Code between `.await`s runs uninterrupted.
Consequence: a long computation or a blocking call in a handler holds the worker thread and
starves all other tasks scheduled on it.

| Don't (blocks the worker)                  | Do                                           |
|--------------------------------------------|----------------------------------------------|
| `std::thread::sleep`                       | `tokio::time::sleep(..).await`               |
| `std::fs::read`                            | `tokio::fs::read(..).await`                  |
| blocking HTTP / DB clients                 | async clients (`reqwest`, `sqlx`)            |
| heavy CPU loop, password hashing           | `tokio::task::spawn_blocking(..).await`      |
| `std::sync::Mutex` held across `.await`    | `tokio::sync::Mutex`, or keep the critical section short and sync |

Rule of thumb: more than ~10–100 µs without an `.await` → consider `spawn_blocking`.

### Concurrency inside a task

```rust
// all must complete; total time = slowest
let (a, b) = tokio::join!(fut_a, fut_b);
let (a, b) = tokio::try_join!(fallible_a, fallible_b)?;   // stops on first Err

// first one wins, the rest are dropped (= cancelled)
tokio::select! {
    v = operation => handle(v),
    _ = tokio::time::sleep(deadline) => timeout(),
}

// shortcut for timeouts
tokio::time::timeout(Duration::from_millis(500), operation).await  // Result<T, Elapsed>
```

`join!`/`select!` run futures concurrently on **one task** (no parallelism).
`tokio::spawn` creates separate tasks that may run **in parallel** on different workers.

### Cancellation

Dropping a future cancels it – it is simply never polled again. Happens in `select!`, `timeout`,
`JoinHandle::abort`, and when a client disconnects (hyper drops the handler future).
Make code *cancel-safe*: don't leave shared state half-updated between two `.await`s
(database transactions help – step 016).

### Synchronization primitives (`tokio::sync`)

| Type                  | Purpose                                          |
|-----------------------|--------------------------------------------------|
| `mpsc` channel        | many producers → one consumer (work queues)      |
| `oneshot` channel     | single value, e.g. response to a request         |
| `broadcast` channel   | every subscriber receives every message          |
| `watch` channel       | latest value only (config reload, shutdown flag) |
| `Mutex`, `RwLock`     | async-aware locks, may be held across `.await`   |
| `Semaphore`           | limit concurrency (e.g. max 10 outgoing calls)   |

### Background jobs

```rust
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;   // first tick is immediate
        do_work().await;
    }
});
```

### Async tests

`#[tokio::test]` creates a `current_thread` runtime per test
(`#[tokio::test(flavor = "multi_thread")]` when needed).

## What changed in this branch

- `src/main.rs` – manual runtime builder, background task `occupancy_reporter`, `Router::merge`
- `src/runtime_demo.rs` – new module: `join!`, `select!`, `spawn_blocking`, blocking anti-pattern, `#[tokio::test]` tests
- `requests/002-tokio-runtime.http` – sample requests

## Try it

```bash
cargo test
cargo run

curl localhost:3000/rooms/1/summary                          # elapsed_ms ≈ 150, not 250
curl -i localhost:3000/rooms/1/availability                  # 200
curl -i "localhost:3000/rooms/1/availability?latency_ms=800" # 504 – deadline wins the race
curl "localhost:3000/reports/occupancy?iterations=200000000" # runs on blocking pool
```

Blocking demo (runtime has 2 worker threads):

```bash
# two blocking calls occupy both workers -> /health waits ~5 s
curl localhost:3000/demo/blocking & curl localhost:3000/demo/blocking & sleep 0.3
time curl localhost:3000/health

# same with the async sleep -> /health answers immediately
curl localhost:3000/demo/non-blocking & curl localhost:3000/demo/non-blocking & sleep 0.3
time curl localhost:3000/health
```

## Exercises

1. Call `compute_report` directly (without `spawn_blocking`) with `iterations=2000000000` and
   observe `/health` while it runs.
2. Rewrite `room_availability` using `tokio::time::timeout` instead of `select!`.
3. Make the background reporter stop after 3 ticks and print its result in `run` by awaiting the `JoinHandle`.
