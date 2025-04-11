import { NextPage } from 'next'
import { useState, useCallback, useEffect, useMemo } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey, Transaction } from '@solana/web3.js'
import { Program } from '@project-serum/anchor'
import { useAnchorProvider } from '@/contexts/AnchorProvider'
import { HauntiClient, ModelMetadata } from '@/client-sdk'
import { WalletMultiButton } from '@/components/WalletMultiButton'
import { toast } from 'react-hot-toast'
import { BN } from 'bn.js'
import { Switch } from '@headlessui/react'
import { create } from 'ipfs-http-client'
import { encryptFile } from '@/utils/crypto'
import { FileUploader } from 'react-drag-drop-files'

const ipfs = create({ 
  url: process.env.NEXT_PUBLIC_IPFS_API!,
  headers: { 
    authorization: `Basic ${Buffer.from(process.env.NEXT_PUBLIC_IPFS_AUTH!).toString('base64')}` 
  }
})

const ModelUploadPage: NextPage = () => {
  const { connection } = useConnection()
  const { publicKey, connected, signTransaction } = useWallet()
  const provider = useAnchorProvider()
  const [program, setProgram] = useState<HauntiClient | null>(null)
  const [uploading, setUploading] = useState(false)
  const [encryption, setEncryption] = useState(true)
  const [modelFile, setModelFile] = useState<File | null>(null)
  const [ipfsHash, setIpfsHash] = useState('')
  const [metadata, setMetadata] = useState<ModelMetadata>({
    name: '',
    description: '',
    license: '',
    price: new BN(0),
    tags: [],
    framework: 'TensorFlow'
  })

  useEffect(() => {
    if (provider) {
      const program = new HauntiClient(provider)
      setProgram(program)
    }
  }, [provider])

  const handleFileChange = useCallback(async (file: File) => {
    setUploading(true)
    try {
      const encryptedFile = encryption ? await encryptFile(file) : file
      const { cid } = await ipfs.add(encryptedFile, {
        progress: (bytes) => {
          toast.loading(`Uploading: ${Math.round(bytes / file.size * 100)}%`, 
            { id: 'upload-progress' }
          )
        }
      })
      
      setIpfsHash(cid.toString())
      setModelFile(file)
      toast.success('File uploaded to IPFS', { id: 'upload-progress' })
    } catch (error) {
      toast.error('File upload failed', { id: 'upload-progress' })
    } finally {
      setUploading(false)
    }
  }, [encryption])

  const handleSubmit = useCallback(async () => {
    if (!program || !publicKey || !signTransaction || !modelFile) return

    const toastId = toast.loading('Creating model NFT...')
    
    try {
      const { transaction, modelPDA } = await program.initializeModel({
        ...metadata,
        modelHash: ipfsHash,
        encrypted: encryption,
        owner: publicKey,
        fileSize: modelFile.size
      })

      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      await connection.confirmTransaction(txid, 'confirmed')
      
      toast.success(`Model NFT created: ${modelPDA.toString()}`, { 
        id: toastId,
        duration: 10000
      })
      
      // Reset form
      setMetadata({
        name: '',
        description: '',
        license: '',
        price: new BN(0),
        tags: [],
        framework: 'TensorFlow'
      })
      setModelFile(null)
      setIpfsHash('')
    } catch (error) {
      toast.error('Model creation failed', { id: toastId })
    }
  }, [program, publicKey, signTransaction, connection, metadata, ipfsHash, modelFile, encryption])

  const validForm = useMemo(() => (
    metadata.name.length > 3 &&
    metadata.description.length > 10 &&
    ipfsHash &&
    modelFile &&
    metadata.price.gtn(0)
  ), [metadata, ipfsHash, modelFile])

  return (
    <div className="min-h-screen bg-slate-900 p-8">
      <div className="max-w-4xl mx-auto">
        <div className="flex items-center justify-between mb-8">
          <h1 className="text-3xl font-bold bg-gradient-to-r from-green-400 to-cyan-500 bg-clip-text text-transparent">
            Upload AI Model
          </h1>
          <WalletMultiButton />
        </div>

        <div className="bg-slate-800/50 rounded-xl p-6 space-y-6">
          {/* Model File Upload */}
          <div className="space-y-4">
            <h2 className="text-xl font-semibold text-white">Model File</h2>
            <FileUploader 
              handleChange={handleFileChange}
              types={['zip', 'h5', 'pt']}
              disabled={uploading || !connected}
            >
              <div className={`border-2 border-dashed rounded-lg p-8 text-center 
                ${uploading ? 'border-cyan-500 bg-cyan-500/10' : 'border-slate-600 hover:border-slate-500'}`}>
                {modelFile ? (
                  <p className="text-slate-300">{modelFile.name}</p>
                ) : (
                  <p className="text-slate-400">
                    {uploading ? 'Uploading...' : 'Drag & Drop or Click to Select'}
                  </p>
                )}
              </div>
            </FileUploader>
            
            {ipfsHash && (
              <p className="text-sm text-slate-400 break-all">
                IPFS CID: {ipfsHash}
              </p>
            )}
          </div>

          {/* Encryption Toggle */}
          <div className="flex items-center space-x-4">
            <Switch
              checked={encryption}
              onChange={setEncryption}
              className={`${
                encryption ? 'bg-cyan-500' : 'bg-slate-700'
              } relative inline-flex h-6 w-11 items-center rounded-full transition-colors`}
            >
              <span className="sr-only">Enable encryption</span>
              <span
                className={`${
                  encryption ? 'translate-x-6' : 'translate-x-1'
                } inline-block h-4 w-4 transform rounded-full bg-white transition-transform`}
              />
            </Switch>
            <span className="text-white">
              FHE Encryption - {encryption ? 'Enabled' : 'Disabled'}
            </span>
          </div>

          {/* Model Metadata Form */}
          <div className="space-y-4">
            <div className="space-y-2">
              <label className="block text-sm font-medium text-white">
                Model Name
              </label>
              <input
                type="text"
                value={metadata.name}
                onChange={(e) => setMetadata({...metadata, name: e.target.value})}
                className="w-full bg-slate-700/50 rounded-lg px-4 py-2 text-white focus:ring-2 focus:ring-cyan-500 outline-none"
                placeholder="My Awesome Model"
              />
            </div>

            <div className="space-y-2">
              <label className="block text-sm font-medium text-white">
                Description
              </label>
              <textarea
                value={metadata.description}
                onChange={(e) => setMetadata({...metadata, description: e.target.value})}
                className="w-full bg-slate-700/50 rounded-lg px-4 py-2 text-white h-32 focus:ring-2 focus:ring-cyan-500 outline-none"
                placeholder="Detailed description of model architecture and capabilities"
              />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <label className="block text-sm font-medium text-white">
                  Price (SOL)
                </label>
                <input
                  type="number"
                  step="0.01"
                  value={metadata.price.divn(1e9).toNumber()}
                  onChange={(e) => setMetadata({...metadata, price: new BN(Number(e.target.value) * 1e9)})}
                  className="w-full bg-slate-700/50 rounded-lg px-4 py-2 text-white focus:ring-2 focus:ring-cyan-500 outline-none"
                  placeholder="0.00"
                />
              </div>

              <div className="space-y-2">
                <label className="block text-sm font-medium text-white">
                  Framework
                </label>
                <select
                  value={metadata.framework}
                  onChange={(e) => setMetadata({...metadata, framework: e.target.value as any})}
                  className="w-full bg-slate-700/50 rounded-lg px-4 py-2 text-white focus:ring-2 focus:ring-cyan-500 outline-none"
                >
                  <option value="TensorFlow">TensorFlow</option>
                  <option value="PyTorch">PyTorch</option>
                  <option value="ONNX">ONNX</option>
                </select>
              </div>
            </div>
          </div>

          {/* Submit Button */}
          <button
            onClick={handleSubmit}
            disabled={!validForm || !connected || uploading}
            className={`w-full py-3 rounded-lg font-medium transition-colors
              ${validForm && connected 
                ? 'bg-cyan-600 hover:bg-cyan-700' 
                : 'bg-slate-800 cursor-not-allowed'}
            `}
          >
            {connected ? 'Publish Model NFT' : 'Connect Wallet to Continue'}
          </button>
        </div>
      </div>
    </div>
  )
}

export default ModelUploadPage
