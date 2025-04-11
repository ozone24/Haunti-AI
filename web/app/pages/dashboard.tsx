import { NextPage } from 'next'
import { useState, useEffect, useCallback } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey, Transaction } from '@solana/web3.js'
import { Program } from '@project-serum/anchor'
import { useAnchorProvider } from '@/contexts/AnchorProvider'
import { HauntiClient, UserStats, TaskProgress } from '@/client-sdk'
import { StatsGrid } from '@/components/dashboard/StatsGrid'
import { EarningsChart } from '@/components/dashboard/EarningsChart'
import { TaskTable } from '@/components/dashboard/TaskTable'
import { Loader } from '@/components/ui/Loader'
import { WalletMultiButton } from '@/components/WalletMultiButton'
import { toast } from 'react-hot-toast'
import { BN } from 'bn.js'
import dynamic from 'next/dynamic'

const PerformanceRadar = dynamic(
  () => import('@/components/dashboard/PerformanceRadar'),
  { ssr: false, loading: () => <Loader size="sm" /> }
)

const DashboardPage: NextPage = () => {
  const { connection } = useConnection()
  const { publicKey, connected, signTransaction } = useWallet()
  const provider = useAnchorProvider()
  const [program, setProgram] = useState<HauntiClient | null>(null)
  const [stats, setStats] = useState<UserStats | null>(null)
  const [activeTasks, setActiveTasks] = useState<TaskProgress[]>([])
  const [completedTasks, setCompletedTasks] = useState<TaskProgress[]>([])
  const [loading, setLoading] = useState(true)
  
  const fetchDashboardData = useCallback(async () => {
    setLoading(true)
    try {
      if (!program || !publicKey) return
      
      const [statsData, tasksData] = await Promise.all([
        program.fetchUserStats(publicKey),
        program.fetchUserTasks(publicKey)
      ])

      setStats(statsData)
      setActiveTasks(tasksData.active)
      setCompletedTasks(tasksData.completed)
    } catch (error) {
      toast.error('Failed to load dashboard data')
    } finally {
      setLoading(false)
    }
  }, [program, publicKey])

  useEffect(() => {
    if (provider) {
      const program = new HauntiClient(provider)
      setProgram(program)
    }
  }, [provider])

  useEffect(() => {
    if (connected) {
      fetchDashboardData()
      const interval = setInterval(fetchDashboardData, 30000)
      return () => clearInterval(interval)
    }
  }, [connected, fetchDashboardData])

  const handleClaimRewards = async () => {
    if (!program || !publicKey || !signTransaction) return

    const toastId = toast.loading('Processing reward claim...')
    
    try {
      const { transaction } = await program.initializeRewardClaim(publicKey)
      
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      await connection.confirmTransaction(txid, 'confirmed')
      
      toast.success('Rewards claimed successfully!', { id: toastId })
      await fetchDashboardData()
    } catch (error) {
      toast.error('Claim failed', { id: toastId })
    }
  }

  if (!connected) {
    return (
      <div className="min-h-screen bg-slate-900 flex items-center justify-center">
        <div className="text-center max-w-md">
          <h1 className="text-3xl font-bold text-white mb-4">
            Connect Wallet to Access Dashboard
          </h1>
          <WalletMultiButton />
        </div>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-slate-900">
      <header className="bg-slate-800/50 border-b border-slate-700">
        <div className="max-w-7xl mx-auto px-4 py-4 flex items-center justify-between">
          <h1 className="text-2xl font-bold text-transparent bg-clip-text bg-gradient-to-r from-green-400 to-cyan-500">
            Haunti AI Dashboard
          </h1>
          <div className="flex items-center space-x-4">
            <WalletMultiButton />
          </div>
        </div>
      </header>

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-8">
        {/* Stats Overview */}
        <section>
          {stats ? (
            <StatsGrid 
              totalEarnings={stats.totalEarnings}
              pendingRewards={stats.pendingRewards}
              activeTasks={stats.activeTaskCount}
              accuracyRating={stats.accuracyRating}
            />
          ) : (
            <div className="animate-pulse grid grid-cols-2 lg:grid-cols-4 gap-4">
              {[...Array(4)].map((_, i) => (
                <div key={i} className="h-32 bg-slate-800 rounded-lg" />
              ))}
            </div>
          )}
        </section>

        {/* Performance Visualization */}
        <section className="grid lg:grid-cols-2 gap-8">
          <div className="bg-slate-800/50 p-6 rounded-xl">
            <h2 className="text-lg font-semibold text-white mb-4">
              Earnings Overview
            </h2>
            <div className="h-64">
              {stats ? (
                <EarningsChart data={stats.earningsHistory} />
              ) : (
                <div className="animate-pulse h-full bg-slate-700/50 rounded-lg" />
              )}
            </div>
          </div>
          
          <div className="bg-slate-800/50 p-6 rounded-xl">
            <h2 className="text-lg font-semibold text-white mb-4">
              Compute Performance
            </h2>
            <div className="h-64">
              {stats ? (
                <PerformanceRadar metrics={stats.performanceMetrics} />
              ) : (
                <div className="animate-pulse h-full bg-slate-700/50 rounded-lg" />
              )}
            </div>
          </div>
        </section>

        {/* Task Management */}
        <section>
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold text-white">
              Active Compute Tasks
            </h2>
            <button
              onClick={handleClaimRewards}
              className="bg-cyan-600 hover:bg-cyan-700 px-4 py-2 rounded-lg text-sm font-medium transition-colors"
              disabled={!stats?.pendingRewards.gtn(0)}
            >
              Claim All Rewards (
                {stats?.pendingRewards.divn(web3.LAMPORTS_PER_SOL).toString() ?? '0'} 
                SOL
              )
            </button>
          </div>
          
          {loading ? (
            <div className="animate-pulse space-y-2">
              {[...Array(3)].map((_, i) => (
                <div key={i} className="h-16 bg-slate-800/50 rounded-lg" />
              ))}
            </div>
          ) : (
            <TaskTable 
              activeTasks={activeTasks}
              completedTasks={completedTasks}
              onTaskSelect={(task) => {/* Implement task detail modal */}
              }
            />
          )}
        </section>
      </main>
    </div>
  )
}

export default DashboardPage
