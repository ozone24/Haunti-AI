#!/usr/bin/env bash
set -euo pipefail

# Configuration
export CLUSTER=${CLUSTER:-mainnet-beta}  # testnet/devnet/mainnet-beta
export RPC_URL=${RPC_URL:-https://api.mainnet-beta.solana.com}
export DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export KEYPAIR_FILE="$DEPLOY_DIR/secrets/${CLUSTER}_deployer.json"
export PROGRAM_ID_FILE="$DEPLOY_DIR/target/deploy/haunti-keypair.json"
export ANCHOR_PROVIDER_URL=$RPC_URL
export CUDA_HOME=/usr/local/cuda  # Update for GPU nodes

# Core Functions
check_dependencies() {
  declare -a required=(
    "solana" "anchor" "cargo" "npm" "node"
    "ipfs" "jq" "aws" "gf_patterns"
  )

  for cmd in "${required[@]}"; do
    if ! command -v $cmd &> /dev/null; then
      echo "❌ Missing required tool: $cmd"
      exit 1
    fi
  done

  echo "✅ Verified all dependencies"
}

setup_environment() {
  mkdir -p "$DEPLOY_DIR/secrets" "$DEPLOY_DIR/artifacts"
  
  # Load or generate deployer keypair
  if [[ ! -f "$KEYPAIR_FILE" ]]; then
    if [[ "$CLUSTER" == "mainnet-beta" ]]; then
      echo "🔒 Mainnet deployment requires HSM/Ledger - set KEYPAIR_FILE"
      exit 1
    else
      solana-keygen new --outfile "$KEYPAIR_FILE" --no-passphrase --force
    fi
  fi

  solana config set --url $RPC_URL --keypair "$KEYPAIR_FILE"
  echo "✅ Environment configured for $CLUSTER"
}

deploy_contracts() {
  (
    cd "$DEPLOY_DIR/blockchain/programs/haunti-core"
    
    # Build with verifiable artifact for SPL governance
    anchor build --verifiable --features $CLUSTER,gpu-accel
    
    # Deploy program
    local program_id=$(jq -r .program_id Anchor.toml)
    solana program deploy \
      --url $RPC_URL \
      --keypair "$KEYPAIR_FILE" \
      --program-id "$PROGRAM_ID_FILE" \
      target/verifiable/haunti.so

    # Verify deployment
    local onchain_id=$(solana program show --program-id $program_id | jq -r .programId)
    if [[ "$onchain_id" != "$program_id" ]]; then
      echo "❌ Program ID mismatch: expected $program_id, got $onchain_id"
      exit 1
    fi

    echo "✅ Contracts deployed to $program_id"
  )
}

setup_frontend() {
  (
    cd "$DEPLOY_DIR/frontend"
    
    # Generate env vars
    local program_id=$(jq -r .program_id ../blockchain/programs/haunti-core/Anchor.toml)
    cat > .env.production <<EOL
VITE_APP_ENV=production
VITE_CLUSTER=$CLUSTER
VITE_PROGRAM_ID=$program_id
VITE_RPC_URL=$RPC_URL
EOL

    # Install & build
    npm ci --omit=dev
    npm run build

    # Deploy to IPFS
    local cid=$(ipfs add -rq dist | tail -n1)
    ipfs pin remote add --service=pinata $cid
    echo "🌐 Frontend deployed to IPFS: https://ipfs.io/ipfs/$cid"

    # Optional: AWS S3 sync
    aws s3 sync dist s3://haunti-frontend-$CLUSTER --delete
  )
}

monitoring_setup() {
  # Prometheus config
  cat <<EOL > prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'solana'
    static_configs:
      - targets: ['localhost:9090']
  - job_name: 'ipfs'
    metrics_path: '/debug/metrics/prometheus'
    static_configs:
      - targets: ['localhost:5001']
EOL

  # Start services
  prometheus --config.file=prometheus.yml &
  grafana-server --homepath /usr/share/grafana &
  echo "📊 Monitoring started: Prometheus (http://localhost:9090) + Grafana (http://localhost:3000)"
}

security_checks() {
  # Smart contract audit
  cargo audit
  cargo crev verify --all-features
  
  # Frontend vuln scan
  npm audit --production
  snyk test ./frontend
  
  # Mainnet-specific checks
  if [[ "$CLUSTER" == "mainnet-beta" ]]; then
    solana-validator verify-deployed $PROGRAM_ID_FILE \
      --rpc-url $RPC_URL \
      --verifier-keypair "$KEYPAIR_FILE"
    
    # Check for phishing domains
    curl -s https://raw.githubusercontent.com/solana-labs/phishing-list/main/src/list.json \
      | jq -e ".whitelist[] | select(. == \"$HOSTNAME\")"
  fi
}

main() {
  case "\$1" in
    full)
      check_dependencies
      setup_environment
      deploy_contracts
      setup_frontend
      monitoring_setup
      security_checks
      ;;
    contracts)
      deploy_contracts
      ;;
    frontend)
      setup_frontend
      ;;
    monitor)
      monitoring_setup
      ;;
    verify)
      security_checks
      ;;
    *)
      echo "Usage: \$0 [full|contracts|frontend|monitor|verify]"
      exit 1
      ;;
  esac
}

main "$@"
