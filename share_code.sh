sshpass -f passwordfile ssh student@10.100.23.$1 "rm -rf sanntid10"
echo "Deleted old project files"
sshpass -f passwordfile scp -r ~/sanntid10 student@10.100.23.$1:sanntid10
echo "Transferred new project files"
