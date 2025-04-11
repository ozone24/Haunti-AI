import { NextPage } from 'next'
import { useState, useEffect, useCallback } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey, Transaction } from '@solana/web3.js'
import { Program } from '@project-serum/anchor'
import { useAnchorProvider } from '@/contexts/AnchorProvider'
import { HauntiClient, MarketTask, TaskCategory } from '@/client-sdk'
import { TaskFilter } from '@/components/market/TaskFilter'
import { TaskList } from '@/components/market/TaskList'
import { Pagination } from '@/components/ui/Pagination'
import { Loader } from '@/components/ui/Loader'
import { WalletMultiButton } from '@/components/WalletMultiButton'
import { useMarketFilters } from '@/hooks/useMarketFilters'
import { toast } from 'react-hot-toast'
import { BN } from 'bn.js'
import dynamic from 'next/dynamic'

const ModelPreview = dynamic(() => import('@/components/ModelPreview'), {
  ssr: false,
  loading: () => <Loader size="sm" />
})

const MarketplacePage: NextPage = () => {
  const { connection } = useConnection()
  const { publicKey, connected, signTransaction } = useWallet()
  const provider = useAnchorProvider()
  const [program, setProgram] = useState<HauntiClient | null>(null)
  const [tasks, setTasks] = useState<MarketTask[]>([])
  const [selectedTask, setSelectedTask] = useState<MarketTask | null>(null)
  const [loading, setLoading] = useState(true)
  const [currentPage, setCurrentPage] = useState(1)
  const [totalPages, setTotalPages] = useState(1)
  
  const {
    filters,
    sortBy,
    searchQuery,
    setCategoryFilter,
    setRewardRange,
    setSortBy,
    setSearchQuery,
    resetFilters
  } = useMarketFilters()

  const fetchMarketTasks = useCallback(async () => {
    setLoading(true)
    try {
      if (!program) return
      
      const response = await program.fetchMarketTasks({
        page: currentPage,
        filters: {
          categories: filters.categories,
          minReward: filters.rewardRange[0] * web3.LAMPORTS_PER_SOL,
          maxReward: filters.rewardRange[1] * web3.LAMPORTS_PER_SOL,
          searchQuery
        },
        sortBy
      })

      setTasks(response.tasks)
      setTotalPages(response.totalPages)
    } catch (error) {
      toast.error('Failed to load market tasks')
    } finally {
      setLoading(false)
    }
  }, [program, currentPage, filters, sortBy, searchQuery])

  useEffect(() => {
    if (provider) {
      const program = new HauntiClient(provider)
      setProgram(program)
    }
  }, [provider])

  useEffect(() => {
    fetchMarketTasks()
  }, [fetchMarketTasks])

  const handleTaskParticipate = async (task: MarketTask) => {
    if (!program || !publicKey || !signTransaction) return

    const toastId = toast.loading('Processing transaction...')
    
    try {
      // Initialize participation
      const { transaction, participationAccount } = 
        await program.initializeParticipation(publicKey, task.account.pubkey)

      // Sign and send transaction
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      
      // Confirm transaction
      await connection.confirmTransaction(txid, 'confirmed')
      
      // Start background processing
      const proofToast = toast.loading('Generating compute proof...', { id: toastId })
      
      // Submit compute proof (handled in worker)
      const result = await program.submitParticipationProof(
        participationAccount,
        task.account.pubkey
      )

      if (result.success) {
        toast.success('Successfully completed task!', { id: proofToast })
      } else {
        toast.error('Verification failed', { id: proofToast })
      }

      await fetchMarketTasks()
    } catch (error) {
      toast.error('Transaction failed', { id: toastId })
    }
  }

  const handlePageChange = (page: number) => {
    setCurrentPage(page)
  }

  return (
    <div className="min-h-screen bg-slate-900">
      <header className="bg-slate-800/50 border-b border-slate-700">
        <div className="max-w-7xl mx-auto px-4 py-4 flex items-center justify-between">
          <h1 className="text-2xl font-bold text-transparent bg-clip-text bg-gradient-to-r from-purple-400 to-indigo-500">
            Haunti AI Marketplace
          </h1>
          <div className="flex items-center space-x-4">
            <WalletMultiButton />
          </div>
        </div>
      </header>

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <div className="grid grid-cols-1 lg:grid-cols-4 gap-8">
          {/* Filters Sidebar */}
          <div className="lg:col-span-1 space-y-6">
            <TaskFilter
              filters={filters}
              sortBy={sortBy}
              searchQuery={searchQuery}
              onCategoryChange={setCategoryFilter}
              onRewardChange={setRewardRange}
              onSortChange={setSortBy}
              onSearchChange={setSearchQuery}
              onReset={resetFilters}
            />
          </div>

          {/* Main Content */}
          <div className="lg:col-span-3">
            {loading ? (
              <div className="flex justify-center py-12">
                <Loader size="lg" />
              </div>
            ) : (
              <>
                <TaskList 
                  tasks={tasks}
                  selectedTask={selectedTask}
                  onSelectTask={setSelectedTask}
                  onParticipate={handleTaskParticipate}
                  connected={connected}
                />
                
                <div className="mt-8">
                  <Pagination
                    currentPage={currentPage}
                    totalPages={totalPages}
                    onPageChange={handlePageChange}
                  />
                </div>
              </>
            )}
          </div>
        </div>

        {/* Preview Panel */}
        {selectedTask && (
          <div className="fixed inset-0 bg-black/50 backdrop-blur z-50">
            <div className="absolute right-0 top-0 h-full w-full max-w-lg bg-slate-800/95 p-6 border-l border-slate-700">
              <div className="flex justify-between items-start mb-6">
                <h2 className="text-xl font-semibold text-white">
                  {selectedTask.account.name}
                </h2>
                <button 
                  onClick={() => setSelectedTask(null)}
                  className="text-slate-400 hover:text-white"
                >
                  ✕
                </button>
              </div>

              <div className="space-y-6">
                <ModelPreview 
                  cid={selectedTask.account.modelCID}
                  encryptedWeightsCID={selectedTask.account.encryptedWeightsCID}
                />

                <div className="grid grid-cols-2 gap-4 text-sm">
                  <div className="bg-slate-700/50 p-4 rounded-lg">
                    <dt className="text-slate-400">Reward</dt>
                    <dd className="text-white font-medium mt-1">
                      {selectedTask.account.reward.divn(web3.LAMPORTS_PER_SOL).toString()} SOL
                    </dd>
                  </div>
                  
                  <div className="bg-slate-700/50 p-4 rounded-lg">
                    <dt className="text-slate-400">Participants</dt>
                    <dd className="text-white font-medium mt-1">
                      {selectedTask.account.participants.toNumber()}
                    </dd>
                  </div>
                </div>

                {connected && (
                  <button
                    onClick={() => handleTaskParticipate(selectedTask)}
                    className="w-full bg-indigo-600 hover:bg-indigo-700 text-white py-3 rounded-lg font-medium transition-colors"
                  >
                    Join Task - Earn {selectedTask.account.reward.divn(web3.LAMPORTS_PER_SOL).toString()} SOL
                  </button>
                )}
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  )
}

export default MarketplacePage
