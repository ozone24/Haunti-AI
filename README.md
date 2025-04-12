# Haunti - Decentralized Verifiable AI Infrastructure [v1.3.0]

**Haunti is an open-source AI infrastructure framework built on Solana, empowering users to self-deploy and manage decentralized AI models through modular toolkits.**

[![AGPL License](https://img.shields.io/badge/license-AGPL--3.0-green)](https://opensource.org/license/agpl-3-0/)
[![Solana Version](https://img.shields.io/badge/Solana-1.17%2B-blue)](https://solana.com)
[![CUDA Requirement](https://img.shields.io/badge/CUDA-12.0%2B-brightgreen)](https://developer.nvidia.com/cuda-toolkit)

## 🌐 Connect with Us

- 🔗 [Website](https://hauntiai.com/)   - 🐦 [Twitter](https://twitter.com/Hauntionx)   - ✍️ [Medium](https://medium.com/@Hauntiai) 

## 🛠 Tech Stack

**Core Components**  
![Rust](https://img.shields.io/badge/Rust-000000?logo=rust)
![Solana](https://img.shields.io/badge/Solana-14F195?logo=solana)
![CUDA](https://img.shields.io/badge/CUDA-76B900?logo=nvidia)
![IPFS](https://img.shields.io/badge/IPFS-65C2CB?logo=ipfs)

**Monitoring**  
![Prometheus](https://img.shields.io/badge/Prometheus-E6522C?logo=prometheus)
![Grafana](https://img.shields.io/badge/Grafana-F46800?logo=grafana)
![Loki](https://img.shields.io/badge/Loki-2C3D50?logo=grafana)

**Deployment**  
![Kubernetes](https://img.shields.io/badge/Kubernetes-326CE5?logo=kubernetes)
![Terraform](https://img.shields.io/badge/Terraform-7B42BC?logo=terraform)

---
## Haunti Architecture
```mermaid
%%{init: {'theme': 'default', 'themeVariables': {
  'primaryColor': '#f8f9fa',
  'primaryBorderColor': '#dee2e6',
  'lineColor': '#adb5bd',
  'textColor': '#212529',
  'clusterBkg': 'transparent'
}}}%%

flowchart TD
    classDef blockchain fill:#4dabf7,stroke:#339af0,color:white;
    classDef privacy fill:#69db7c,stroke:#40c057,color:black;
    classDef storage fill:#ff922b,stroke:#f76707,color:black;
    classDef monitoring fill:#9775fa,stroke:#7c4dff,color:white;

    subgraph BL["Blockchain Layer"]
        BC_Registry[[Model Registry]]:::blockchain
        BC_Marketplace[[Compute Marketplace]]:::blockchain
        BC_Verifier[[ZK Verifier]]:::blockchain
    end

    subgraph PL["Privacy Layer"]
        PV_FHE[[FHE Runtime]]:::privacy
        PV_ZK[[ZK Prover Cluster]]:::privacy
        PV_Enclave[[Trusted Enclaves]]:::privacy
    end

    subgraph SL["Storage Layer"]
        ST_IPFS[[IPFS Cluster]]:::storage
        ST_Arweave[[Arweave Permastore]]:::storage
        ST_Cache[[Proof Cache]]:::storage
    end

    subgraph ML["Monitoring Layer"]
        MO_Prom[[Prometheus]]:::monitoring
        MO_Grafana[[Grafana]]:::monitoring
        MO_Alert[[AlertManager]]:::monitoring
    end

    BC_Registry -->|Model Metadata| ST_IPFS
    PV_FHE -->|Encrypted Gradients| BC_Verifier
    PV_ZK -->|Proof Artifacts| ST_Arweave
    MO_Prom -->|Metrics| MO_Grafana
    BC_Marketplace -->|Task Scheduling| PV_Enclave
    MO_Alert -->|Auto-scale| PV_ZK
          
```
---

## 🌟 Key Innovations

### 1. **Cryptographic AI Integrity Layer**
   - **Hybrid Proof System**: Combine ZK-SNARKs (Plonky3) and FHE (TFHE) for end-to-end verifiability
   - **Multi-Chain Attestation**: Cross-chain state proofs via Wormhole/ICS
   - **Data Lineage**: Immutable dataset provenance using IPFS+Arweave

### 2. **Decentralized Compute Network**
   - **GPU Orchestration**: Kubernetes-based scheduling with NVIDIA MIG support
   - **Elastic Scaling**: Auto-scale GPU pods based on ZK proof complexity
   - **Fault Tolerance**: Byzantine-resistant task replication

### 3. **Enterprise Security Suite**
   - **Hardened Runtime**: SGX/TEE support for sensitive operations
   - **Compliance Ready**: HIPAA/GDPR-compatible data handling
   - **Zero-Trust Architecture**: SPIFFE-based service identity

---

## 🏗 System Architecture

### Core Components

| Component | Tech Stack | Function |
|-----------|------------|----------|
| **Solana Programs** | Rust/Anchor | On-chain verification & governance |
| **ZK Prover Cluster** | Plonky3/CUDA | GPU-accelerated proof generation |
| **FHE Runtime** | TFHE-rs/C++ | Encrypted model operations |
| **Data Layer** | IPFS/Arweave/Ceramic | Decentralized storage with provenance |
| **Monitoring** | Prometheus/Loki/Tempo | Distributed tracing & metrics |

![Component Diagram](https://docs.haunti.ai/component-diagram-v2.svg)

---

## 🛠 Installation & Configuration

### Hardware Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| GPU | NVIDIA T4 (8GB) | A100 (40GB) |
| CPU | 4 cores | 16 cores EPYC |
| Memory | 32GB DDR4 | 256GB DDR5 |
| Storage | 500GB NVMe | 10TB NVMe RAID |

### 1. Base System Setup

```bash
# Ubuntu 22.04 LTS
sudo apt install -y \
  nvidia-cuda-toolkit \
  nvidia-docker2 \
  containerd.io \
  solana-cli=1.17.5

# Configure NVIDIA Container Runtime
sudo nvidia-ctk runtime configure --runtime=containerd
sudo systemctl restart containerd
```

### Run Local Network
```
# Start validator with GPU support
solana-test-validator --gossip-host 127.0.0.1 \
  --geyser-plugin-config config/geyser.yml \
  --rpc-port 8899 \
  --enable-cpi-and-log-storage

# Deploy programs
anchor deploy --provider.cluster localhost
```

## 📚 Usage Examples

### 1. Encrypted Model Training
```
let model = EncryptedModel::new(fhe_scheme::TFHE)?;
let dataset = Dataset::from_ipfs(cid)?;
let result = model.train(
  &dataset,
  TrainingConfig {
    epochs: 100,
    batch_size: 32,
    proof_type: ZkProofType::Plonky3
  }
)?;
```

## 2. Cross-chain Inference
```
const proof = await generateInferenceProof(
  modelCID, 
  inputData, 
  {useGPU: true}
);

const tx = await program.methods
  .verifyInference(proof)
  .accounts({model: modelPDA})
  .rpc();
```


## ⚙ Configuration
### Key Files

| Path | Purpose |
|-----------|---------|
| config/prometheus.yml | Metrics scraping rules |
| deploy/terraform/main.tf | Cloud provisioning |
| anchor/programs/haunti | Solana program suite |
| web/app/utils/zkproof.ts | Web3 proof handling |

### Environment Variables
```
# .env.example
SOLANA_RPC="https://api.mainnet-beta.solana.com"
IPFS_API="https://ipfs.haunti.ai:5001"
CUDA_DEVICES="0,1" # GPU indices
ZK_CIRCUITS_PATH="./circuits"
```

## 🤝 Contributing
### Development Workflow
```
# 1. Create feature branch
git checkout -b feat/awesome-feature

# 2. Run tests
cargo test --features test,zk \
  && anchor test --skip-local-validator

# 3. Submit PR
gh pr create --fill --reviewer "@haunti/core-team"
```

### Code Standards
- Rust: Clippy lvl strict
- TS: Airbnb style + ESLint
- Commit: Conventional commits
- Docs: Rustdoc -> mdBook

## 📜 License
### AGPL-3.0 © Haunti Foundation

### Commercial licensing available for enterprise use.
