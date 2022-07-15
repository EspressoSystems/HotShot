set -ex

# Taken from .gitlab-ci.yml
# Uses nix-shell to generate the HotShot.pdf

cd ./src
nix-shell --run "make"
cd ..
mv ./src/HotShot.pdf .
