sshpass -f passwordfile ssh student@10.100.23.20 "rm -rf sanntid_10"
echo "Deleted old project files"
sshpass -f passwordfile scp -r ~/sanntid10 student@10.100.23.20:sanntid_10
echo "Transferred new project files"
