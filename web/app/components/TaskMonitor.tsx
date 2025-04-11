import { NextPage } from 'next'
import { useState, useEffect, useCallback, useMemo } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey, Transaction } from '@solana/web3.js'
import { Program } from '@project-serum/anchor'
import { BarChart, LineChart } from '@visx/xychart'
import { scaleLinear, scaleTime } from '@visx/scale'
import { Group } from '@visx/group'
import { AxisBottom, AxisLeft } from '@visx/axis'
import { Legend } from '@visx/legend'
import { useAnchorProvider } from '@/contexts/AnchorProvider'
import { HauntiClient, ComputeTask, TaskState } from '@/client-sdk'
import { WalletMultiButton } from '@/components/WalletMultiButton'
import { toast } from 'react-hot-toast'
import { BN } from 'bn.js'
import { formatDuration, intervalToDuration } from 'date-fns'

const TaskMonitorPage: NextPage = () => {
  const { connection } = useConnection()
  const { publicKey, connected, signTransaction } = useWallet()
  const provider = useAnchorProvider()
  const [program, setProgram] = useState<HauntiClient | null>(null)
  const [tasks, setTasks] = useState<ComputeTask[]>([])
  const [selectedTask, setSelectedTask] = useState<PublicKey | null>(null)
  const [autoRefresh, setAutoRefresh] = useState(true)

  const fetchTasks = useCallback(async () => {
    if (!program || !publicKey) return
    
    try {
      const tasks = await program.fetchTasksByOwner(publicKey)
      setTasks(tasks.sort((a, b) => 
        b.timestamp.sub(a.timestamp).toNumber()
      ))
    } catch (error) {
      toast.error('Failed to fetch tasks')
    }
  }, [program, publicKey])

  useEffect(() => {
    if (autoRefresh) {
      const interval = setInterval(fetchTasks, 10000)
      return () => clearInterval(interval)
    }
  }, [autoRefresh, fetchTasks])

  useEffect(() => {
    if (provider) {
      const program = new HauntiClient(provider)
      setProgram(program)
      fetchTasks()
    }
  }, [provider, fetchTasks])

  const handleCancelTask = useCallback(async (taskPDA: PublicKey) => {
    if (!program || !publicKey || !signTransaction) return

    const toastId = toast.loading('Cancelling task...')
    try {
      const { transaction } = await program.cancelTask(taskPDA, publicKey)
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      await connection.confirmTransaction(txid, 'confirmed')
      await fetchTasks()
      toast.success('Task cancelled successfully', { id: toastId })
    } catch (error) {
      toast.error('Failed to cancel task', { id: toastId })
    }
  }, [program, publicKey, signTransaction, connection, fetchTasks])

  const selectedTaskData = useMemo(() => 
    tasks.find(t => t.publicKey.equals(selectedTask || PublicKey.default)),
  [tasks, selectedTask])

  const resourceScale = useMemo(() => 
    scaleLinear<number>({
      domain: [0, selectedTaskData?.maxResources.gpuMem || 1],
      range: [0, 400],
    }),
  [selectedTaskData])

  const timeScale = useMemo(() => 
    scaleTime({
      domain: [
        selectedTaskData?.timestamp.toNumber() ?
          new Date(selectedTaskData.timestamp.toNumber() * 1000) : new Date(),
        new Date()
      ],
      range: [0, 600],
    }),
  [selectedTaskData])

  return (
    <div className="min-h-screen bg-slate-900 p-8">
      <div className="max-w-6xl mx-auto">
        <div className="flex items-center justify-between mb-8">
          <h1 className="text-3xl font-bold bg-gradient-to-r from-purple-400 to-pink-500 bg-clip-text text-transparent">
            AI Task Monitor
          </h1>
          <div className="flex items-center gap-4">
            <WalletMultiButton />
            <label className="flex items-center space-x-2">
              <input
                type="checkbox"
                checked={autoRefresh}
                onChange={(e) => setAutoRefresh(e.target.checked)}
                className="form-checkbox h-4 w-4 text-purple-500"
              />
              <span className="text-white">Auto Refresh</span>
            </label>
          </div>
        </div>

        <div className="grid grid-cols-3 gap-6">
          {/* Task List */}
          <div className="col-span-1 bg-slate-800/50 rounded-xl p-4 space-y-4">
            <h2 className="text-xl font-semibold text-white">Active Tasks</h2>
            <div className="space-y-2">
              {tasks.map(task => (
                <div
                  key={task.publicKey.toString()}
                  onClick={() => setSelectedTask(task.publicKey)}
                  className={`p-4 rounded-lg cursor-pointer transition-colors ${
                    selectedTask?.equals(task.publicKey) 
                      ? 'bg-purple-500/20 border border-purple-500'
                      : 'bg-slate-700/50 hover:bg-slate-700'
                  }`}
                >
                  <div className="flex justify-between items-center">
                    <div>
                      <p className="font-medium text-white">{task.taskType}</p>
                      <p className="text-sm text-slate-400">
                        {new Date(task.timestamp.toNumber() * 1000).toLocaleString()}
                      </p>
                    </div>
                    <span className={`px-2 py-1 rounded-full text-sm ${
                      task.state === TaskState.Running 
                        ? 'bg-green-500/20 text-green-400'
                        : task.state === TaskState.Failed
                        ? 'bg-red-500/20 text-red-400'
                        : 'bg-slate-600 text-slate-300'
                    }`}>
                      {TaskState[task.state]}
                    </span>
                  </div>
                </div>
              ))}
              {tasks.length === 0 && (
                <p className="text-center text-slate-500 py-4">
                  No active tasks found
                </p>
              )}
            </div>
          </div>

          {/* Task Details */}
          <div className="col-span-2 space-y-6">
            {selectedTaskData ? (
              <>
                <div className="bg-slate-800/50 rounded-xl p-6">
                  <div className="flex justify-between items-start mb-6">
                    <div>
                      <h2 className="text-2xl font-bold text-white">
                        {selectedTaskData.taskType} Task
                      </h2>
                      <p className="text-slate-400">
                        Created {formatDuration(intervalToDuration({
                          start: new Date(selectedTaskData.timestamp.toNumber() * 1000),
                          end: new Date()
                        }))} ago
                      </p>
                    </div>
                    <div className="space-x-4">
                      {selectedTaskData.state === TaskState.Running && (
                        <button
                          onClick={() => handleCancelTask(selectedTaskData.publicKey)}
                          className="px-4 py-2 bg-red-500/20 text-red-400 rounded-lg hover:bg-red-500/30 transition-colors"
                        >
                          Cancel Task
                        </button>
                      )}
                    </div>
                  </div>

                  {/* Resource Charts */}
                  <div className="space-y-8">
                    <div>
                      <h3 className="text-lg font-semibold text-white mb-4">
                        Resource Utilization
                      </h3>
                      <div className="h-64">
                        <svg width="100%" height="100%">
                          <Group>
                            <AxisLeft
                              scale={resourceScale}
                              stroke="#475569"
                              tickStroke="#475569"
                              tickLabelProps={() => ({
                                fill: '#94a3b8',
                                fontSize: 12,
                                textAnchor: 'end',
                              })}
                            />
                            <AxisBottom
                              top={200}
                              scale={timeScale}
                              stroke="#475569"
                              tickStroke="#475569"
                              tickLabelProps={() => ({
                                fill: '#94a3b8',
                                fontSize: 12,
                                textAnchor: 'middle',
                              })}
                            />
                            {selectedTaskData.resourceLogs.map((log, i) => (
                              <rect
                                key={i}
                                x={timeScale(new Date(log.timestamp * 1000))}
                                y={200 - resourceScale(log.gpuMem)}
                                width={4}
                                height={resourceScale(log.gpuMem)}
                                fill="#8b5cf6"
                              />
                            ))}
                          </Group>
                        </svg>
                      </div>
                    </div>

                    {/* Task Metadata */}
                    <div className="grid grid-cols-2 gap-4">
                      <div className="space-y-2">
                        <p className="text-slate-400">Compute Units</p>
                        <p className="text-white">
                          {selectedTaskData.computeUnits.toNumber()}
                        </p>
                      </div>
                      <div className="space-y-2">
                        <p className="text-slate-400">Result CID</p>
                        <p className="text-white break-all text-sm">
                          {selectedTaskData.resultCID || 'Pending...'}
                        </p>
                      </div>
                    </div>
                  </div>
                </div>
              </>
            ) : (
              <div className="bg-slate-800/50 rounded-xl p-8 text-center">
                <p className="text-slate-500">
                  Select a task to view detailed metrics
                </p>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

export default TaskMonitorPage
