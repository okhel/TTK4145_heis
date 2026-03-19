use std::collections::{HashMap, VecDeque};
use crate::types::Position;
use crate::order_management::types::{NextOrderResult, Order};
use crate::elevator::elevio::poll::{CallButton, CallType};
use colored::Colorize;

use super::order_list::OrderList;

pub fn assign_new_order(
    order: Order,
    order_list: &mut OrderList,
    positions: &HashMap<usize, Position>,
    alive_elevs: &[usize],
) -> Option<usize> {
    if order_list.contains(&order) {
        return None;
    }

    order_list.set_queue(rebuild_queue(order_list.queue(), order.clone()));
    let (busy_elevs, available_elevs) =
        designate_busy_idle(alive_elevs.to_vec(), order_list, order.clone(), positions);

    if let Some((elev_idx, paused_order)) =
        pause_order(busy_elevs.clone(), &order, order_list, positions)
    {
        order_list.push_front(paused_order);
        order_list.dequeue(&order);
        order_list.assign(elev_idx, order);
        return Some(elev_idx);
    }

    if let Some(closest) = find_closest_elev(available_elevs.clone(), &order, positions) {
        order_list.assign(closest, order.clone());
        order_list.dequeue(&order);
        return Some(closest);
    }
    else if !available_elevs.is_empty() { println!("No available elevator for: {}", order); }

    None
}

pub fn assign_next_order(
    completed: Order,
    order_list: &mut OrderList,
) -> NextOrderResult {
    let mut result = NextOrderResult { next: None, clear: None };
    let eligible = get_eligible_orders(order_list.queue(), completed.clone());

    match completed.cb.call {
        CallType::HallUp | CallType::HallDown => {
            resolve_hall_direction(&mut result, &eligible, &completed);
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
        let order = find_closest_order_otw(
            result.next.as_ref().unwrap().clone(),
            eligible,
            completed.clone(),
        );
        order_list.dequeue(&order);
        order_list.assign(completed.elev_idx, order.clone());
        result.next = Some(order);
    }
    else if !eligible.is_empty() { println!("{}", format!("Could not assign next order after: {}, eligible: [{}]", completed, eligible.iter().map(|o| format!("{}", o)).collect::<Vec<_>>().join(", ")).red().bold()); }

    // overwrite result to give order to the relevant elevator, not the one that registered the order
    if let Some(ref mut order) = result.next {
        order.elev_idx = completed.elev_idx;
    }
    result
}

fn get_eligible_orders(orders: &VecDeque<Order>, completed: Order) -> Vec<Order> {
    orders
        .iter()
        .filter(|o| o.is_for(completed.elev_idx))
        .cloned()
        .collect()
}

fn designate_busy_idle(
    alive_elevs: Vec<usize>,
    order_list: &OrderList,
    order: Order,
    positions: &HashMap<usize, Position>,
) -> (Vec<usize>, Vec<usize>) {
    let busy: Vec<usize> = alive_elevs.iter().copied()
        .filter(|&i| order_list.has_assignment(i) && positions.get(&i).is_some_and(|p| !p.obstruction))
        .collect();
    let idle: Vec<usize> = alive_elevs.iter().copied()
        .filter(|&i| !busy.contains(&i) && order.is_for(i) && positions.get(&i).is_some_and(|p| !p.obstruction))
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

    (is_below && on_way_down && new_order.is_for(elev_idx))
        || (is_above && on_way_up && new_order.is_for(elev_idx))
}

fn find_closest_elev(
    candidates: Vec<usize>,
    order: &Order,
    positions: &HashMap<usize, Position>,
) -> Option<usize> {
    candidates.into_iter()
        .filter_map(|idx| positions.get(&idx).map(|pos| (idx, pos)))
        .min_by_key(|&(_, state)| u8::abs_diff(state.floor, order.cb.floor))
        .map(|(idx, _)| idx)
}

fn pause_order(
    busy_elevs: Vec<usize>,
    order: &Order,
    order_list: &OrderList,
    positions: &HashMap<usize, Position>,
) -> Option<(usize, Order)> {
    busy_elevs.into_iter().find_map(|idx| {
        let curr = order_list.current_order(idx)?.clone();
        let pos = positions.get(&idx)?;
        if order_on_the_way(idx, pos.floor, curr.clone(), order.clone()) {
            Some((idx, curr))
        } else {
            None
        }
    })
}

fn find_closest_order_otw(target: Order, eligible: Vec<Order>, completed: Order) -> Order {
    eligible.into_iter()
        .filter(|o| order_on_the_way(completed.elev_idx, completed.cb.floor, target.clone(), o.clone()))
        .min_by_key(|o| u8::abs_diff(completed.cb.floor, o.cb.floor))
        .unwrap_or(target)
}

fn opposite_hall(dir: CallType) -> CallType {
    match dir {
        CallType::HallUp => CallType::HallDown,
        CallType::HallDown => CallType::HallUp,
        CallType::Cab => unreachable!(),
    }
}

fn resolve_hall_direction(result: &mut NextOrderResult, eligible: &[Order], completed: &Order) {
    let dir = completed.cb.call;
    if let Some(cab) = find_cab_order(eligible.to_vec(), completed.cb.floor, dir) {
        result.next = Some(cab);
    } else {
        match should_change_direction(eligible.first().cloned(), completed.cb.floor, dir) {
            Some(true) => result.next = Some(Order {
                cb: CallButton { floor: completed.cb.floor, call: opposite_hall(dir) },
                elev_idx: completed.elev_idx,
            }),
            Some(false) => {
                result.next = eligible.first().cloned();
                result.clear = Some(Order {
                    cb: CallButton { floor: completed.cb.floor, call: dir },
                    elev_idx: completed.elev_idx,
                });
            }
            None => {}
        }
    }
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
