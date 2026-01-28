sshpass -f passwordfile ssh student@10.100.23.$1 "pkill elevatorserver"
sshpass -f passwordfile ssh student@10.100.23.$1 "bash -lc 'elevatorserver & cd sanntid10/TTK4145_heis/Exercise3/single_elevator && cargo run'" &

sshpass -f passwordfile ssh student@10.100.23.$2 "pkill elevatorserver"
sshpass -f passwordfile ssh student@10.100.23.$2 "bash -lc 'elevatorserver & cd sanntid10/TTK4145_heis/Exercise3/single_elevator && cargo run'" & 

sshpass -f passwordfile ssh student@10.100.23.$3 "pkill elevatorserver"
sshpass -f passwordfile ssh student@10.100.23.$3 "bash -lc 'elevatorserver & cd sanntid10/TTK4145_heis/Exercise3/single_elevator && cargo run'"
