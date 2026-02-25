use crate::elevator::elevio::poll::CallButton as CallButton;

NUM_ELEVATORS = 3;

pub enum OrderPhase {
    New = 0,
    Confirmed = 1,
    Complete = 2,
}

pub struct Order {
    pub call: CallButton,
    pub owner: usize,

    pub phase: OrderPhase,
    pub confirm_acks: u8,
    pub complete_acks: u8,
}

// small helper func for only changing this elevator's bits 
fn bit_for(id: usize) -> u8 {
    1u8 << id
}


let all = (1u8 << NUM_ELEVATORS) - 1;  // simple bitmask to check if all elevators agree on a order state

let mut order = Order {
    call,
    owner: my_id,
    phase: OrderPhase::New,
    confirm_acks: 0,
    complete_acks: 0,
};

// new order locally 
order.confirm_acks |= bit_for(my_id);

// store it locally
orders.insert(order.call, order);

// broadcast full state (including the orders maybe?)
broadcast_state();


fn merge_order(local: &mut Order, remote: &Order, all: u8) {
    // union acks
    local.confirm_acks |= remote.confirm_acks;
    local.complete_acks |= remote.complete_acks;


    local.phase = std::cmp::max(local.phase, remote.phase); // get highest phase

    // check if all elevators agree on confirm order, if so upgrade you local phase 
    if local.confirm_acks == all && local.phase < OrderPhase::Confirmed {
        println!("Barrier hit for confirms!");
        local.phase = OrderPhase::Confirmed;
    }

    // same but with complete 
    if local.complete_acks == all {
        println!("Barrier hit for complete!");
        local.phase = OrderPhase::Complete;
    }
}

 // if new order elevator hasnt seen yet, acknowledge it and broadcast 
if order.phase == OrderPhase::New {
    if (order.confirm_acks & bit_for(my_id)) == 0 {
        order.confirm_acks |= bit_for(my_id);
        broadcast_state();
    }
}


// when elevator completes order (specifically owner)
if order.owner == my_id && order.phase == OrderPhase::Confirmed {
    // locally completed
    order.complete_acks |= bit_for(my_id);
    broadcast_state();
}

// when other elevators receive that complete:
if order.phase >= OrderPhase::Confirmed {
    if (order.complete_acks & bit_for(my_id)) == 0 {
        // acknowledge completion
        order.complete_acks |= bit_for(my_id);
        broadcast_state();
    }
}


// if everyone agrees order is complete, finally delete it from our orders and maybe broadcast again 
if order.phase == OrderPhase::Complete && order.complete_acks == all {
    println!("Clearing order {:?}", order.call);
    orders.remove(&order.call);
    broadcast_state();
}



