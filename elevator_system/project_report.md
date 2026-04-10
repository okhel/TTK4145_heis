# Elevator System -- Group 10

## Design Description

Our system is a distributed elevator controller written in Rust, running on the Tokio async runtime. It controls up to three elevators across four floors, communicating over UDP on a local network. The architecture consists of four concurrent Tokio tasks spawned from `main.rs`, connected exclusively through `mpsc` and `broadcast` channels -- there is no shared mutable state between tasks.

**Tasks and their roles:**

- **`elevator_runner`** -- Interfaces with the elevator hardware (or simulator) via the Elevio TCP protocol. Spawns sub-tasks for polling buttons, floor sensors, and obstruction, plus a motor FSM and a light controller. Emits `ElevatorEvent`s (button presses, state updates, order completions) and receives `ElevatorCommand`s (assign order, set light).
- **`network_runner`** -- Handles all UDP communication: heartbeat pings for liveness detection, and a two-layer reliable transport for order messages. Routes outbound messages based on role (master fans out to all peers; slave sends only to master).
- **`order_manager`** -- The central decision-maker. Maintains the order queue, current assignments, elevator positions, and role (master/slave). Translates between elevator events, network events, and the assignment heuristics.
- **`store_online_elevators`** -- Merges heartbeat pings into a membership list using a `BTreeMap<id, Instant>` with a 3-second timeout. Broadcasts the sorted alive list to both the network and order manager tasks.

The Elevio driver was originally provided as a blocking script. We translated its polling loops into async Tokio tasks (`poll.rs`) that use `tokio::time::sleep` for periodic sampling at 25 ms intervals, while the underlying TCP I/O (`elev.rs`) remains synchronous behind an `Arc<Mutex<TcpStream>>`. This lets multiple polling tasks (buttons, floor sensor, obstruction) run concurrently within the Tokio runtime without dedicated OS threads.

**Master election and failover:**
The lowest online elevator ID is the master. When the alive set changes, each node re-evaluates its role. A new master kickstarts all known elevators with work requests and inherits any in-flight orders from lost nodes by re-queuing them. State and queue synchronization messages are sent to newly joined nodes.

**Fault tolerance mechanisms:**
An order watchdog (15 s) re-queues stuck assignments. An idle watchdog (3 s) prompts the master to assign queued work to idle elevators. Lost elevators have their current orders re-injected into the assignment pipeline. A receive-side deduplication window (3 s) prevents double-processing. On panic, the process aborts immediately so the heartbeat timeout triggers peer-side recovery.

<!-- FIGURE 1: Module interaction diagram (replace with actual figure) -->

*Figure 1: Module interaction diagram. Four Tokio tasks communicate through typed channels. The order manager is the sole bridge between the elevator and network domains.*

<!-- FIGURE 2: Order lifecycle (replace with actual figure) -->

*Figure 2: Order lifecycle. A button press produces a `RequestOrder` sent to the master. After all peers acknowledge (`AckComplete`), the master broadcasts `QueueOrders` (turning on lights), runs assignment, and sends `AssignOrders` to the chosen elevator. On arrival, the elevator sends `CompleteOrder`; the master assigns the next order and broadcasts `ClearOrders` to turn off lights.*

## Case Study 1: Custom Order Assignment Heuristics

### The decision

Rather than using a single scalar cost function (e.g., the provided hall-request-assigner pattern where `cost = f(distance, direction, load)`), we built a multi-heuristic assignment system in `assignment.rs` that directly encodes elevator scheduling intuitions.

### What we implemented

When a new order arrives, `assign_new_order` splits online elevators into busy and idle sets (excluding obstructed elevators), then applies two strategies in sequence:

1. **Preemption** (`pause_order`): If a busy elevator is currently traveling past the new hall call "on the way" -- determined geometrically by `order_on_the_way`, which checks floor ordering and call direction -- its current order is stashed at the front of the queue and the new order is assigned instead. This avoids the elevator overshooting a stop it could have served.

2. **Closest idle** (`find_closest_elev`): Among idle elevators, pick the one minimizing `abs_diff(floor, order.floor)`.

After an order is completed, `assign_next_order` selects the next order using directional logic: it prefers cab orders in the current travel direction, checks whether a direction reversal is warranted, and refines the choice with `find_closest_order_otw` to pick the nearest eligible "on the way" order. The queue itself is structured with cab orders before hall orders (`rebuild_queue`).

### Alternative not chosen

The standard approach is a unified cost function: compute a numeric score for each elevator-order pair (incorporating distance, direction penalty, current load) and assign to the minimum. This is conceptually simpler, more extensible, and easier to reason about formally.

### Why we chose our approach

Our heuristic system directly encodes the scheduling behaviors we wanted -- particularly preemption, which is awkward to express as a cost term. It avoids tuning arbitrary penalty weights, which we found hard to get right during development. The tradeoff is maintainability: adding a new consideration means modifying control flow rather than adding a cost term.

### Reflection

The preemption logic is effective but can cause order thrashing when multiple orders arrive in quick succession, since each new "on the way" order can preempt the previous assignment. In hindsight, a cost function with a "switching penalty" would handle this more gracefully. Specifically, `pause_order` could be replaced by comparing `cost(keep_current)` vs. `cost(preempt_and_requeue)`, which would naturally dampen rapid reassignments. This is the single change we would prioritize for a future iteration.

## Case Study 2: Reliable UDP with Two-Layer Acking

### The decision

We built a custom reliable transport over UDP rather than using TCP or an existing messaging library.

### What we implemented

The transport has two layers:

**Layer 1 -- Per-datagram reliability** (`transport.rs`): Each `Msg` is wrapped in a `Frame::Data { seq, msg }` and serialized with bincode. The sender retries every 5 ms until it receives a `Frame::Ack` matching both the sequence number *and* the full message content, or gives up after 10,000 attempts. The receiver sends three copies of each ACK to reduce ACK-loss probability. Each send spawns its own Tokio task with an ephemeral UDP socket, so multiple reliable sends proceed concurrently.

**Layer 2 -- Application-level multicast completion** (`networking.rs`, `pending.rs`): Each outbound message is assigned a sequence number and a `PendingMap` entry tracking which peers must complete their Layer-1 sends. Only when all targeted peers succeed (or are removed due to going offline) does the order manager receive `AckComplete`. This guarantees that the master does not assign an order until all peers have received the queue update.

Failure handling adapts to role: if a slave's send to the master fails, it retargets to the new `master_id` in the pending set. Otherwise, up to 20 application-level retries are attempted before giving up and resolving the peer to avoid deadlock.

### Alternative not chosen

TCP provides reliable, ordered delivery out of the box, eliminating both layers. We also considered existing Rust crates for reliable UDP (e.g., `laminar`).

### Why we chose UDP + custom acking

Three properties drove the decision:

- **Dynamic routing**: Our master-slave topology changes at runtime. A slave must retarget mid-flight to a new master if the current one dies. UDP's connectionless model lets us switch destination addresses per-message without teardown/reconnection.
- **No head-of-line blocking**: Our messages are independent; a delayed message should not block subsequent ones. TCP's ordered stream would stall everything behind a lost packet.
- **Explicit failure semantics**: `handle_send_failure` implements role-aware retry logic (slave retargets, master resolves) that would be hard to express through TCP's opaque retry/timeout behavior.

The tradeoff is complexity: we effectively reimplemented parts of TCP's reliability guarantees.

### Reflection

The 5 ms ACK timeout works well on a local lab network but is fragile on higher-latency links. Adaptive timeouts with exponential backoff would be a straightforward improvement. Additionally, matching ACKs by full `Msg` equality (rather than just sequence number) provides strong consistency but increases bandwidth; a lightweight hash would achieve the same safety at lower cost. Despite the added complexity, building this layer gave us fine-grained control that proved valuable during debugging -- every retry, timeout, and retarget is visible in our logs.