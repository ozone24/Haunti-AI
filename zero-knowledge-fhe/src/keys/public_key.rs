//! Cryptographic public key management for Haunti infrastructure
//! Integrates with Solana Ed25519 and ZKP systems

use {
    ed25519_dalek::{PublicKey as EdPublicKey, Signature, Verifier},
    solana_program::{pubkey::Pubkey as SolanaPubkey, program_error::ProgramError},
    ark_ec::{AffineCurve, ProjectiveCurve},
    ark_ed25519::{EdwardsAffine, Fr},
    ark_ff::{PrimeField, ToBytes},
    ark_serialize::{CanonicalDeserialize, CanonicalSerialize, SerializationError},
    std::{convert::TryFrom, str::FromStr},
    thiserror::Error,
};

/// Main public key structure supporting multiple cryptographic systems
#[derive(Debug, Clone, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub enum HauntiPublicKey {
    /// Standard Ed25519 public key (used by Solana)
    Ed25519(EdPublicKey),
    /// ZKP System public key (e.g., for Groth16 proofs)
    ZKPGroth(EdwardsAffine),
    /// Hierarchical Deterministic (HD) derived key
    HD {
        master: EdPublicKey,
        derivation_path: Vec<u32>,
    },
}

#[derive(Error, Debug)]
pub enum PublicKeyError {
    #[error("Invalid public key format")]
    InvalidFormat,
    #[error("Public key verification failed")]
    VerificationFailure,
    #[error("ZK proof system error")]
    ZKPError(#[from] ark_serialize::SerializationError),
    #[error("Solana program error")]
    SolanaError(#[from] ProgramError),
}

impl HauntiPublicKey {
    /// Generate from Solana's native Pubkey type
    pub fn from_solana(pubkey: &SolanaPubkey) -> Self {
        let bytes = pubkey.to_bytes();
        let ed_key = EdPublicKey::from_bytes(&bytes).unwrap();
        HauntiPublicKey::Ed25519(ed_key)
    }

    /// Convert to Solana Pubkey
    pub fn to_solana(&self) -> Result<SolanaPubkey, PublicKeyError> {
        match self {
            Self::Ed25519(ed) => Ok(SolanaPubkey::new_from_array(ed.to_bytes())),
            _ => Err(PublicKeyError::InvalidFormat),
        }
    }

    /// Verify a signature against message
    pub fn verify(&self, msg: &[u8], signature: &[u8]) -> Result<(), PublicKeyError> {
        match self {
            Self::Ed25519(ed_pubkey) => {
                let sig = Signature::from_bytes(signature).map_err(|_| PublicKeyError::InvalidFormat)?;
                ed_pubkey.verify(msg, &sig).map_err(|_| PublicKeyError::VerificationFailure)
            }
            Self::ZKPGroth(affine) => {
                // ZKP verification logic (e.g., Groth16)
                let sig = EdwardsAffine::deserialize(&mut &*signature)?;
                let msg_field = Fr::from_be_bytes_mod_order(msg);
                // Placeholder for actual ZKP verification
                if affine.into_projective() == sig.into_projective().mul(msg_field) {
                    Ok(())
                } else {
                    Err(PublicKeyError::VerificationFailure)
                }
            }
            Self::HD { master, derivation_path } => {
                // HD key derivation verification
                let derived = self.derive_child(0)?; // Simplified example
                derived.verify(msg, signature)
            }
        }
    }

    /// Derive child public key for HD wallets
    pub fn derive_child(&self, index: u32) -> Result<Self, PublicKeyError> {
        match self {
            Self::HD { master, derivation_path } => {
                let mut new_path = derivation_path.clone();
                new_path.push(index);
                Ok(HauntiPublicKey::HD {
                    master: master.clone(),
                    derivation_path: new_path,
                })
            }
            _ => Err(PublicKeyError::InvalidFormat),
        }
    }

    /// Generate ZKP public key from parameters
    pub fn from_zkp_params<C: AffineCurve>(params: &C) -> Self 
    where
        C: CanonicalSerialize + CanonicalDeserialize,
    {
        HauntiPublicKey::ZKPGroth(EdwardsAffine::prime_subgroup_generator().into())
    }
}

impl TryFrom<&[u8]> for HauntiPublicKey {
    type Error = PublicKeyError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() == 32 {
            EdPublicKey::from_bytes(bytes)
                .map(Self::Ed25519)
                .map_err(|_| PublicKeyError::InvalidFormat)
        } else {
            EdwardsAffine::deserialize(&mut &*bytes)
                .map(Self::ZKPGroth)
                .map_err(|e| PublicKeyError::ZKPError(e))
        }
    }
}

impl ToBytes for HauntiPublicKey {
    fn write<W: std::io::Write>(&self, writer: W) -> Result<(), SerializationError> {
        match self {
            Self::Ed25519(k) => k.to_bytes().as_ref().write(writer),
            Self::ZKPGroth(k) => k.serialize_compressed(writer),
            Self::HD { master, derivation_path } => {
                master.to_bytes().as_ref().write(&mut writer)?;
                for seg in derivation_path {
                    seg.write(writer)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Keypair;

    #[test]
    fn test_ed25519_roundtrip() {
        let keypair = Keypair::generate(&mut rand::rngs::OsRng);
        let pubkey = HauntiPublicKey::Ed25519(keypair.public);
        
        let bytes = pubkey.to_bytes().unwrap();
        let decoded = HauntiPublicKey::try_from(bytes.as_slice()).unwrap();
        
        assert_eq!(pubkey, decoded);
    }

    #[test]
    fn test_solana_integration() {
        let solana_pubkey = SolanaPubkey::new_unique();
        let haunti_key = HauntiPublicKey::from_solana(&solana_pubkey);
        let converted_back = haunti_key.to_solana().unwrap();
        
        assert_eq!(solana_pubkey, converted_back);
    }

    #[test]
    fn test_hd_derivation() {
        let master_key = HauntiPublicKey::Ed25519(Keypair::generate(&mut rand::rngs::OsRng).public);
        let hd_key = HauntiPublicKey::HD {
            master: master_key.clone().try_into().unwrap(),
            derivation_path: vec![44, 501, 0],
        };
        
        let child_key = hd_key.derive_child(1).unwrap();
        match child_key {
            HauntiPublicKey::HD { derivation_path, .. } => {
                assert_eq!(derivation_path, vec![44, 501, 0, 1]);
            }
            _ => panic!("Invalid key type"),
        }
    }
}
