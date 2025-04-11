#!/usr/bin/env bash
set -eo pipefail

# Configuration
export CLUSTER="devnet"
export RPC_URL="http://localhost:8899"
export IPFS_API_PORT=5001
export IPFS_GATEWAY_PORT=8080
export PROMETHEUS_PORT=9090
export GRAFANA_PORT=3000
export SECURITY_DIR="${HOME}/.haunti-secure"
export TEST_ACCOUNTS=5  # Number of test accounts to generate
export HAUNTI_TOKENS=1000000  # Initial token supply per account

# Core Functions
check_dependencies() {
  declare -a required=(
    "solana" "anchor" "cargo" "npm" "node"
    "ipfs" "jq" "docker" "lsof" "curl"
  )

  for cmd in "${required[@]}"; do
    if ! command -v "$cmd" &> /dev/null; then
      echo "❌ Missing required tool: $cmd"
      exit 1
    fi
  done
  echo "✅ Verified all dependencies"
}

clean_environment() {
  pkill -f "solana-test-validator" || true
  pkill -f "ipfs daemon" || true
  docker stop haunti-monitoring &> /dev/null || true
  rm -rf "${SECURITY_DIR}"
  mkdir -p "${SECURITY_DIR}/keys" "${SECURITY_DIR}/ipfs"
  echo "🧹 Cleaned previous environment"
}

start_solana() {
  if lsof -i :8899; then
    echo "🔄 Restarting Solana test validator..."
    pkill -f "solana-test-validator"
  fi
  
  solana-test-validator \
    --reset \
    --rpc-port 8899 \
    --faucet-port 9900 \
    --quiet \
    --account-dir "${SECURITY_DIR}/accounts" \
    --bpf-program Haunti1111111111111111111111111111111111111 ../blockchain/programs/haunti-core/target/deploy/haunti.so \
    --bpf-program Token11111111111111111111111111111111111111 $(solana program show --program-id TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA) \
    > "${SECURITY_DIR}/solana.log" 2>&1 &
  
  sleep 5
  solana config set --url "$RPC_URL"
  echo "🚀 Solana $CLUSTER initialized"
}

deploy_contracts() {
  (
    cd "../blockchain/programs/haunti-core"
    anchor build --features devnet,gpu-accel
    solana program deploy --url "$RPC_URL" target/deploy/haunti.so
    local program_id=$(solana program show --program-id Haunti1111111111111111111111111111111111111 | jq -r .programId)
    echo "📦 Contracts deployed: $program_id"
  )
}

setup_ipfs() {
  ipfs init --profile=test --empty-repo --repo="${SECURITY_DIR}/ipfs"
  ipfs config Addresses.API "/ip4/0.0.0.0/tcp/${IPFS_API_PORT}"
  ipfs config Addresses.Gateway "/ip4/0.0.0.0/tcp/${IPFS_GATEWAY_PORT}"
  ipfs daemon --enable-pubsub-experiment --enable-namesys-pubsub \
    > "${SECURITY_DIR}/ipfs.log" 2>&1 &
  
  until curl -s "http://localhost:${IPFS_API_PORT}/api/v0/version" > /dev/null; do
    sleep 1
  done
  echo "🌐 IPFS node ready (API:${IPFS_API_PORT}, Gateway:${IPFS_GATEWAY_PORT})"
}

start_monitoring() {
  docker run -d --name haunti-monitoring \
    -p "${PROMETHEUS_PORT}:9090" \
    -p "${GRAFANA_PORT}:3000" \
    -v "${SECURITY_DIR}/prometheus.yml:/etc/prometheus/prometheus.yml" \
    prom/cloudmonitoring:latest
  
  echo "📊 Monitoring stack started:"
  echo "   - Prometheus: http://localhost:${PROMETHEUS_PORT}"
  echo "   - Grafana: http://localhost:${GRAFANA_PORT} (admin:admin)"
}

fund_test_accounts() {
  for i in $(seq 1 "$TEST_ACCOUNTS"); do
    keyfile="${SECURITY_DIR}/keys/test_account_${i}.json"
    solana-keygen new --no-passphrase --silent --outfile "$keyfile"
    address=$(solana address --keypair "$keyfile")
    solana airdrop 100 "$address" || solana transfer "$address" 100 --allow-unfunded-recipient
    echo "💰 Funded account ${i}: ${address}"
  done
  
  # Distribute Haunti tokens
  local mint_address=$(solana address --keypair "../blockchain/programs/haunti-core/target/deploy/haunti-keypair.json")
  for keyfile in "${SECURITY_DIR}"/keys/test_account_*.json; do
    spl-token create-account "$mint_address" --owner "$(solana address --keypair "$keyfile")"
    spl-token mint "$mint_address" "$HAUNTI_TOKENS" --owner "../blockchain/programs/haunti-core/target/deploy/haunti-keypair.json"
  done
  echo "🪙 Distributed $HAUNTI_TOKENS HAUNTI tokens to test accounts"
}

health_check() {
  echo "🩺 Running health checks..."
  
  # Solana node
  curl -s -X POST -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
    "$RPC_URL" | jq -e '.result == "ok"'
  
  # IPFS node
  test_cid=$(echo "Haunti Devnet" | ipfs add -Q)
  ipfs cat "$test_cid" | grep -q "Haunti Devnet"
  
  # Monitoring
  nc -z localhost "$PROMETHEUS_PORT"
  nc -z localhost "$GRAFANA_PORT"
  
  echo "✅ All services operational"
}

enable_gpu() {
  if lspci | grep -i 'nvidia'; then
    export CUDA_HOME=/usr/local/cuda
    echo "🎮 Detected NVIDIA GPU - CUDA enabled"
  else
    echo "⚠️  No NVIDIA GPU detected - Falling back to CPU mode"
  fi
}

main() {
  check_dependencies
  clean_environment
  enable_gpu
  start_solana
  deploy_contracts
  setup_ipfs
  start_monitoring
  fund_test_accounts
  health_check
}

main "$@"
