import { NextPage } from 'next'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey, Transaction } from '@solana/web3.js'
import { Program, Provider, web3 } from '@project-serum/anchor'
import { useState, useEffect, useCallback } from 'react'
import { toast, Toaster } from 'react-hot-toast'
import dynamic from 'next/dynamic'
import { useAnchorProvider } from '../contexts/AnchorProvider'
import { HauntiClient, Task } from '../client-sdk'
import { TaskCreationModal } from '../components/TaskCreation'
import { TaskCard } from '../components/TaskCard'
import { Loader } from '../components/ui/Loader'
import { ClusterNetworkSwitch } from '../components/ClusterSwitch'
import { useTaskMarket } from '../hooks/useTaskMarket'
import { WalletMultiButton } from '../components/WalletMultiButton'
import { FHEModelPreview } from '../components/fhe/FHEModelPreview'

// Dynamically load heavy components
const ModelVisualizer = dynamic(() => import('../components/ModelVisualizer'), {
  ssr: false,
  loading: () => <Loader size="md" />
})

const Home: NextPage = () => {
  const { connection } = useConnection()
  const { publicKey, connected, signTransaction } = useWallet()
  const provider = useAnchorProvider()
  const [program, setProgram] = useState<HauntiClient | null>(null)
  const [userTasks, setUserTasks] = useState<Task[]>([])
  const [marketTasks, setMarketTasks] = useState<Task[]>([])
  const [selectedTask, setSelectedTask] = useState<Task | null>(null)
  const [showCreationModal, setShowCreationModal] = useState(false)
  const { fetchMarketTasks } = useTaskMarket()

  // Initialize program instance
  useEffect(() => {
    if (provider) {
      const program = new HauntiClient(provider)
      setProgram(program)
    }
  }, [provider])

  // Fetch user-specific tasks
  const fetchUserTasks = useCallback(async () => {
    if (program && publicKey) {
      try {
        const tasks = await program.fetchTasksByUser(publicKey)
        setUserTasks(tasks)
      } catch (error) {
        toast.error('Failed to load user tasks')
      }
    }
  }, [program, publicKey])

  // Fetch public market tasks
  const fetchPublicTasks = useCallback(async () => {
    try {
      const tasks = await fetchMarketTasks()
      setMarketTasks(tasks)
    } catch (error) {
      toast.error('Failed to load market tasks')
    }
  }, [fetchMarketTasks])

  // Handle task creation
  const handleTaskSubmit = async (taskConfig: {
    modelCID: string
    datasetCID: string
    reward: number
    isPublic: boolean
  }) => {
    if (!program || !publicKey) return

    const toastId = toast.loading('Creating task on blockchain...')
    
    try {
      const tx = await program.createTask(
        publicKey,
        taskConfig.modelCID,
        taskConfig.datasetCID,
        new BN(taskConfig.reward * web3.LAMPORTS_PER_SOL),
        taskConfig.isPublic
      )

      const signedTx = await signTransaction!(tx)
      const txId = await connection.sendRawTransaction(signedTx.serialize())
      
      await connection.confirmTransaction(txId, 'confirmed')
      await fetchUserTasks()
      
      toast.success('Task created successfully!', { id: toastId })
      setShowCreationModal(false)
    } catch (error) {
      toast.error('Transaction failed', { id: toastId })
    }
  }

  // Handle task participation
  const handleTaskJoin = async (task: Task) => {
    if (!program || !publicKey) return

    const toastId = toast.loading('Joining AI training task...')
    
    try {
      const tx = await program.joinTask(publicKey, task.publicKey)
      const signedTx = await signTransaction!(tx)
      const txId = await connection.sendRawTransaction(signedTx.serialize())
      
      await connection.confirmTransaction(txId, 'confirmed')
      await fetchUserTasks()
      
      toast.success('Successfully joined task!', { id: toastId })
    } catch (error) {
      toast.error('Failed to join task', { id: toastId })
    }
  }

  // Subscribe to real-time updates
  useEffect(() => {
    let subscriptionId: number | null = null

    if (program) {
      subscriptionId = connection.onProgramAccountChange(
        program.programId,
        async () => {
          await Promise.all([fetchUserTasks(), fetchPublicTasks()])
        },
        'confirmed'
      )
    }

    return () => {
      if (subscriptionId) connection.removeProgramAccountChangeListener(subscriptionId)
    }
  }, [connection, program, fetchUserTasks, fetchPublicTasks])

  return (
    <div className="min-h-screen bg-gradient-to-b from-slate-900 to-slate-800">
      <nav className="border-b border-slate-700 bg-slate-900/50 backdrop-blur">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex items-center justify-between h-16">
            <div className="flex items-center space-x-8">
              <span className="text-2xl font-bold bg-gradient-to-r from-purple-400 to-indigo-500 bg-clip-text text-transparent">
                Haunti AI
              </span>
              <ClusterNetworkSwitch />
            </div>
            <WalletMultiButton />
          </div>
        </div>
      </nav>

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <div className="flex justify-between items-center mb-8">
          <h2 className="text-2xl font-semibold text-white">AI Compute Marketplace</h2>
          <button
            onClick={() => setShowCreationModal(true)}
            className="bg-indigo-600 hover:bg-indigo-700 text-white px-6 py-2 rounded-lg font-medium disabled:opacity-50"
            disabled={!connected}
          >
            New Task
          </button>
        </div>

        {showCreationModal && (
          <TaskCreationModal
            onClose={() => setShowCreationModal(false)}
            onSubmit={handleTaskSubmit}
          />
        )}

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
          <div className="lg:col-span-2 space-y-6">
            <section>
              <h3 className="text-lg font-medium text-white mb-4">Your Active Tasks</h3>
              <div className="space-y-4">
                {userTasks.length === 0 ? (
                  <div className="text-slate-400">
                    {connected ? 'No active tasks' : 'Connect wallet to view tasks'}
                  </div>
                ) : (
                  userTasks.map(task => (
                    <TaskCard
                      key={task.publicKey.toString()}
                      task={task}
                      onSelect={setSelectedTask}
                      onAction={handleTaskJoin}
                    />
                  ))
                )}
              </div>
            </section>

            <section>
              <h3 className="text-lg font-medium text-white mb-4">Public Market</h3>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {marketTasks.map(task => (
                  <TaskCard
                    key={task.publicKey.toString()}
                    task={task}
                    onSelect={setSelectedTask}
                    onAction={handleTaskJoin}
                    isPublic
                  />
                ))}
              </div>
            </section>
          </div>

          <div className="lg:col-span-1">
            <section className="sticky top-4">
              {selectedTask ? (
                <div className="bg-slate-800/50 p-6 rounded-xl border border-slate-700">
                  <h3 className="text-xl font-semibold text-white mb-4">
                    Task Details
                  </h3>
                  <ModelVisualizer cid={selectedTask.modelCID} />
                  <FHEModelPreview 
                    publicKey={selectedTask.publicKey}
                    cid={selectedTask.encryptedParamsCID}
                  />
                </div>
              ) : (
                <div className="text-slate-400 text-center py-12">
                  Select a task to view details
                </div>
              )}
            </section>
          </div>
        </div>
      </main>

      <Toaster position="bottom-right" toastOptions={{ duration: 5000 }} />
    </div>
  )
}

export default Home
