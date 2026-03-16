use std::collections::{HashMap, VecDeque};
use crate::order_management::types::{NextOrderResult, Order};
use crate::elevator::elevio::poll::{CallButton, CallType};
use colored::Colorize;


pub fn is_mine(order: Order, idx: usize) -> bool {
    order.elev_idx == idx || order.cb.call != CallType::Cab
}

pub fn assign_new_order(
    order: Order,
    orders: &mut VecDeque<Order>,
    positions: &HashMap<usize, u8>,
    current_orders: &mut HashMap<usize, Option<Order>>,
    alive_elevs: &[usize],
) -> Option<usize> {
    if orders.iter().any(|o| o == &order) {
        return None;
    }

    *orders = rebuild_queue(orders, order.clone());
    let (busy_elevs, available_elevs) =
        designate_busy_idle(alive_elevs.to_vec(), current_orders, order.clone());

    if let Some((elev_idx, paused_order)) =
        find_order_otw(busy_elevs.clone(), &order, current_orders, positions)
    {
        orders.push_front(paused_order);
        orders.retain(|o| o != &order);
        current_orders.insert(elev_idx, Some(order));
        return Some(elev_idx);
    }

    if let Some(closest) = find_closest_elev(available_elevs.clone(), &order, positions) {
        current_orders.insert(closest, Some(order.clone()));
        orders.retain(|o| o != &order);
        return Some(closest);
    }
    else if !available_elevs.is_empty() { println!("No available elevator for: {}", order); }

    None
}

pub fn assign_next_order(
    completed: Order,
    orders: &mut VecDeque<Order>,
    current_orders: &mut HashMap<usize, Option<Order>>,
) -> NextOrderResult {
    let mut result = NextOrderResult { next: None, clear: None };
    let eligible = get_eligible_orders(orders, completed.clone());

    match completed.cb.call {
        CallType::HallUp => {
            if let Some(cab) = find_cab_order(eligible.clone(), completed.cb.floor, CallType::HallUp) {
                result.next = Some(cab);
            } else {
                match should_change_direction(eligible.first().cloned(), completed.cb.floor, CallType::HallUp) {
                    Some(true) => result.next = Some(Order {
                        cb: CallButton { floor: completed.cb.floor, call: CallType::HallDown },
                        elev_idx: completed.elev_idx,
                    }),
                    Some(false) => {
                        result.next = eligible.first().cloned();
                        result.clear = Some(Order {
                            cb: CallButton { floor: completed.cb.floor, call: CallType::HallUp },
                            elev_idx: completed.elev_idx,
                        });
                    }
                    None => {}
                }
            }
        }

        CallType::HallDown => {
            if let Some(cab) = find_cab_order(eligible.clone(), completed.cb.floor, CallType::HallDown) {
                result.next = Some(cab);
            } else {
                match should_change_direction(eligible.first().cloned(), completed.cb.floor, CallType::HallDown) {
                    Some(true) => result.next = Some(Order {
                        cb: CallButton { floor: completed.cb.floor, call: CallType::HallUp },
                        elev_idx: completed.elev_idx,
                    }),
                    Some(false) => {
                        result.next = eligible.first().cloned();
                        result.clear = Some(Order {
                            cb: CallButton { floor: completed.cb.floor, call: CallType::HallDown },
                            elev_idx: completed.elev_idx,
                        });
                    }
                    None => {}
                }
            }
        }

        CallType::Cab => {
            if let Some(order) = eligible.first().cloned() {
                if order.cb.floor > completed.cb.floor {
                    result.clear = Some(Order {
                        cb: CallButton { floor: completed.cb.floor, call: CallType::HallUp },
                        elev_idx: completed.elev_idx,
                    });
                }
                else if order.cb.floor < completed.cb.floor {
                    result.clear = Some(Order {
                        cb: CallButton { floor: completed.cb.floor, call: CallType::HallDown },
                        elev_idx: completed.elev_idx,
                    });
                }
                result.next = Some(order);
            }
        }
    }

    if result.next.is_some() {
        let order = find_closest_order(
            result.next.as_ref().unwrap().clone(),
            eligible,
            completed.clone(),
        );
        orders.retain(|o| o != &order);
        current_orders.insert(completed.elev_idx, Some(order.clone()));
        result.next = Some(order);
    }
    else if !eligible.is_empty() { println!("{}", format!("Could not assign next order after: {}, eligible: [{}]", completed, eligible.iter().map(|o| format!("{}", o)).collect::<Vec<_>>().join(", ")).red().bold()); }

    // overwrite result to give order to the relevant elevator, not the one that registered the order 
    if let Some(ref mut order) = result.next {
        order.elev_idx = completed.elev_idx;
    }
    result
}


fn elevator_may_take_order(elev_idx: usize, order: &Order) -> bool {
    order.cb.call != CallType::Cab || order.elev_idx == elev_idx
}

fn get_eligible_orders(orders: &VecDeque<Order>, completed: Order) -> Vec<Order> {
    orders
        .iter()
        .filter(|o| elevator_may_take_order(completed.elev_idx, o))
        .cloned()
        .collect()
}

fn designate_busy_idle(
    alive_elevs: Vec<usize>,
    current_orders: &HashMap<usize, Option<Order>>,
    order: Order,
) -> (Vec<usize>, Vec<usize>) {
    let busy: Vec<usize> = alive_elevs.iter().copied()
        .filter(|&i| current_orders.get(&i).and_then(|o| o.as_ref()).is_some())
        .collect();
    let idle: Vec<usize> = alive_elevs.iter().copied()
        .filter(|&i| !busy.contains(&i) && elevator_may_take_order(i, &order))
        .collect();
    (busy, idle)
}

fn rebuild_queue(orders: &VecDeque<Order>, order: Order) -> VecDeque<Order> {
    let mut cab_orders: VecDeque<Order> = orders.iter().filter(|o| o.cb.call == CallType::Cab).cloned().collect();
    let mut hall_orders: VecDeque<Order> = orders.iter().filter(|o| o.cb.call != CallType::Cab).cloned().collect();
    let mut new_orders = VecDeque::with_capacity(orders.len() + 1);
    new_orders.append(&mut cab_orders);
    new_orders.append(&mut hall_orders);
    new_orders.push_back(order);
    new_orders
}

fn order_on_the_way(elev_idx: usize, position: u8, curr_order: Order, new_order: Order) -> bool {
    let new_floor = new_order.cb.floor;
    let curr_floor = curr_order.cb.floor;
    let new_call = new_order.cb.call;

    let is_below = curr_floor <= new_floor && new_floor < position;
    let is_above = curr_floor >= new_floor && new_floor > position;
    let on_way_down = new_call == CallType::HallDown || new_call == CallType::Cab;
    let on_way_up = new_call == CallType::HallUp || new_call == CallType::Cab;

    (is_below && on_way_down && elevator_may_take_order(elev_idx, &new_order))
        || (is_above && on_way_up && elevator_may_take_order(elev_idx, &new_order))
}

fn find_closest_elev(
    candidates: Vec<usize>,
    order: &Order,
    positions: &HashMap<usize, u8>,
) -> Option<usize> {
    candidates.into_iter()
        .filter_map(|idx| positions.get(&idx).map(|&pos| (idx, pos)))
        .min_by_key(|&(_, pos)| u8::abs_diff(pos, order.cb.floor))
        .map(|(idx, _)| idx)
}

fn find_order_otw(
    busy_elevs: Vec<usize>,
    order: &Order,
    current_orders: &HashMap<usize, Option<Order>>,
    positions: &HashMap<usize, u8>,
) -> Option<(usize, Order)> {
    busy_elevs.into_iter().find_map(|idx| {
        let curr = current_orders.get(&idx)?.as_ref()?.clone();
        let &pos = positions.get(&idx)?;
        if order_on_the_way(idx, pos, curr.clone(), order.clone()) {
            Some((idx, curr))
        } else {
            None
        }
    })
}

fn find_closest_order(target: Order, eligible: Vec<Order>, completed: Order) -> Order {
    eligible.into_iter()
        .filter(|o| order_on_the_way(completed.elev_idx, completed.cb.floor, target.clone(), o.clone()))
        .min_by_key(|o| u8::abs_diff(completed.cb.floor, o.cb.floor))
        .unwrap_or(target)
}

fn find_cab_order(orders: Vec<Order>, floor: u8, dir: CallType) -> Option<Order> {
    orders.into_iter().find(|o| {
        o.cb.call == CallType::Cab && match dir {
            CallType::HallUp   => o.cb.floor > floor,
            CallType::HallDown => o.cb.floor < floor,
            CallType::Cab      => false,
        }
    })
}

fn should_change_direction(order: Option<Order>, floor: u8, dir: CallType) -> Option<bool> {
    let order = order?;
    Some(match dir {
        CallType::HallUp   => order.cb.floor < floor,
        CallType::HallDown => order.cb.floor > floor,
        CallType::Cab      => false,
    })
}