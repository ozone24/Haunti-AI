import { NextPage } from 'next'
import { useState, useEffect, useCallback, useMemo } from 'react'
import { useConnection, useWallet } from '@solana/wallet-adapter-react'
import { PublicKey, Transaction } from '@solana/web3.js'
import useSWR from 'swr'
import { Disclosure, Tab, Transition } from '@headlessui/react'
import { ChevronUpIcon, ChartBarIcon, DocumentPlusIcon, LockOpenIcon } from '@heroicons/react/24/outline'
import { GovernanceClient, ProposalState, VoteType } from '@haunti/governance-sdk'
import { BN } from 'bn.js'
import { toast } from 'react-toastify'

const GOVERNANCE_PROGRAM_ID = new PublicKey(process.env.NEXT_PUBLIC_GOVERNANCE_PROGRAM_ID!)
const HAUNTI_TOKEN_MINT = new PublicKey(process.env.NEXT_PUBLIC_TOKEN_MINT!)

const Governance: NextPage = () => {
  const { connection } = useConnection()
  const { publicKey, signTransaction } = useWallet()
  const [activeTab, setActiveTab] = useState(0)
  const [newProposal, setNewProposal] = useState({ title: '', description: '', ipfsDetails: '' })
  const [stakeAmount, setStakeAmount] = useState('')
  const [delegateAddress, setDelegateAddress] = useState('')

  // Governance client initialization
  const governanceClient = useMemo(() => 
    new GovernanceClient(connection, GOVERNANCE_PROGRAM_ID), [connection])

  // SWR data fetchers
  const { data: proposals } = useSWR('proposals', async () => 
    governanceClient.getProposals({ programId: GOVERNANCE_PROGRAM_ID }), 
    { refreshInterval: 10000 }
  )

  const { data: votingPower } = useSWR(publicKey ? 'voting-power' : null, async () => 
    governanceClient.getVoterPower(publicKey!)
  )

  const { data: userStake } = useSWR(publicKey ? 'user-stake' : null, async () => 
    governanceClient.getTokenStake(publicKey!, HAUNTI_TOKEN_MINT)
  )

  // Proposal creation handler
  const createProposal = async () => {
    if (!publicKey || !signTransaction) return
    
    try {
      const transaction = new Transaction()
      const [proposalTx, proposalPda] = await governanceClient.createProposal({
        creator: publicKey,
        title: newProposal.title,
        description: newProposal.description,
        metadataUri: newProposal.ipfsDetails,
        voteType: VoteType.SINGLE_CHOICE
      })
      
      transaction.add(proposalTx)
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      
      await connection.confirmTransaction(txid)
      toast.success('Proposal created successfully!')
      setNewProposal({ title: '', description: '', ipfsDetails: '' })
    } catch (error) {
      toast.error(`Proposal creation failed: ${error.message}`)
    }
  }

  // Voting handler
  const castVote = async (proposalId: PublicKey, approve: boolean) => {
    if (!publicKey || !signTransaction) return
    
    try {
      const transaction = new Transaction()
      const voteTx = await governanceClient.castVote({
        voter: publicKey,
        proposal: proposalId,
        amount: votingPower!,
        approve
      })
      
      transaction.add(voteTx)
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      
      await connection.confirmTransaction(txid)
      toast.success(`Vote ${approve ? 'for' : 'against'} submitted!`)
    } catch (error) {
      toast.error(`Voting failed: ${error.message}`)
    }
  }

  // Token staking handler
  const stakeTokens = async () => {
    if (!publicKey || !signTransaction || !stakeAmount) return
    
    try {
      const amount = new BN(parseFloat(stakeAmount) * 1e6)
      const transaction = new Transaction()
      const stakeTx = await governanceClient.stakeTokens({
        owner: publicKey,
        tokenMint: HAUNTI_TOKEN_MINT,
        amount
      })
      
      transaction.add(stakeTx)
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      
      await connection.confirmTransaction(txid)
      toast.success('Tokens staked successfully!')
      setStakeAmount('')
    } catch (error) {
      toast.error(`Staking failed: ${error.message}`)
    }
  }

  // Delegation handler
  const delegateVotingPower = async () => {
    if (!publicKey || !signTransaction || !delegateAddress) return
    
    try {
      const delegatePubkey = new PublicKey(delegateAddress)
      const transaction = new Transaction()
      const delegateTx = await governanceClient.delegateVotes({
        delegator: publicKey,
        delegate: delegatePubkey
      })
      
      transaction.add(delegateTx)
      const signedTx = await signTransaction(transaction)
      const txid = await connection.sendRawTransaction(signedTx.serialize())
      
      await connection.confirmTransaction(txid)
      toast.success('Voting power delegated!')
      setDelegateAddress('')
    } catch (error) {
      toast.error(`Delegation failed: ${error.message}`)
    }
  }

  return (
    <div className="max-w-7xl mx-auto px-4 py-8">
      {/* Governance Header */}
      <div className="bg-indigo-900 rounded-lg p-6 mb-8 shadow-xl">
        <h1 className="text-4xl font-bold text-white mb-4">Haunti DAO Governance</h1>
        <div className="grid grid-cols-3 gap-6 text-white">
          <div className="bg-indigo-800 p-4 rounded-lg">
            <h3 className="text-sm opacity-75 mb-1">Voting Power</h3>
            <p className="text-2xl font-mono">
              {votingPower ? (votingPower.toNumber() / 1e6).toLocaleString() : '--'} HAUNT
            </p>
          </div>
          <div className="bg-indigo-800 p-4 rounded-lg">
            <h3 className="text-sm opacity-75 mb-1">Staked Tokens</h3>
            <p className="text-2xl font-mono">
              {userStake ? (userStake.amount.toNumber() / 1e6).toLocaleString() : '--'} HAUNT
            </p>
          </div>
          <div className="bg-indigo-800 p-4 rounded-lg">
            <h3 className="text-sm opacity-75 mb-1">Active Proposals</h3>
            <p className="text-2xl font-mono">
              {proposals?.filter(p => p.state === ProposalState.ACTIVE).length || 0}
            </p>
          </div>
        </div>
      </div>

      {/* Governance Tabs */}
      <Tab.Group selectedIndex={activeTab} onChange={setActiveTab}>
        <Tab.List className="flex space-x-2 mb-8 border-b border-gray-200">
          {['Proposals', 'New Proposal', 'Stake Tokens', 'Delegate'].map((tab, idx) => (
            <Tab
              key={tab}
              className={({ selected }) => 
                `px-4 py-2 text-sm font-medium rounded-t-lg ${
                  selected 
                    ? 'bg-indigo-100 text-indigo-700'
                    : 'text-gray-500 hover:text-gray-700'
                }`
              }
            >
              {tab}
            </Tab>
          ))}
        </Tab.List>

        <Tab.Panels>
          {/* Proposals List */}
          <Tab.Panel>
            <div className="space-y-4">
              {proposals?.map(proposal => (
                <Disclosure key={proposal.publicKey.toBase58()}>
                  {({ open }) => (
                    <>
                      <Disclosure.Button className="flex justify-between w-full px-4 py-3 bg-white rounded-lg shadow-sm border border-gray-200 hover:border-indigo-200">
                        <div className="text-left">
                          <h3 className="font-medium">{proposal.title}</h3>
                          <span className={`text-sm px-2 py-1 rounded ${
                            proposal.state === ProposalState.ACTIVE 
                              ? 'bg-green-100 text-green-800'
                              : 'bg-gray-100 text-gray-600'
                          }`}>
                            {ProposalState[proposal.state]}
                          </span>
                        </div>
                        <ChevronUpIcon className={`${
                          open ? 'transform rotate-180' : ''
                        } w-5 h-5 text-indigo-500`} />
                      </Disclosure.Button>
                      <Transition
                        enter="transition duration-100 ease-out"
                        enterFrom="transform scale-95 opacity-0"
                        enterTo="transform scale-100 opacity-100"
                        leave="transition duration-75 ease-out"
                        leaveFrom="transform scale-100 opacity-100"
                        leaveTo="transform scale-95 opacity-0"
                      >
                        <Disclosure.Panel className="px-4 pt-4 pb-2 bg-gray-50 rounded-b-lg">
                          <div className="mb-4">
                            <p className="text-gray-600">{proposal.description}</p>
                            <div className="mt-4 grid grid-cols-2 gap-4">
                              <div className="bg-white p-4 rounded-lg shadow">
                                <h4 className="text-sm font-medium text-gray-500">For Votes</h4>
                                <p className="text-2xl text-green-600">
                                  {(proposal.forVotes.toNumber() / 1e6).toLocaleString()}
                                </p>
                              </div>
                              <div className="bg-white p-4 rounded-lg shadow">
                                <h4 className="text-sm font-medium text-gray-500">Against Votes</h4>
                                <p className="text-2xl text-red-600">
                                  {(proposal.againstVotes.toNumber() / 1e6).toLocaleString()}
                                </p>
                              </div>
                            </div>
                          </div>
                          {proposal.state === ProposalState.ACTIVE && (
                            <div className="flex space-x-3">
                              <button
                                onClick={() => castVote(proposal.publicKey, true)}
                                className="px-4 py-2 bg-green-100 text-green-700 rounded-lg hover:bg-green-200"
                              >
                                Vote For
                              </button>
                              <button
                                onClick={() => castVote(proposal.publicKey, false)}
                                className="px-4 py-2 bg-red-100 text-red-700 rounded-lg hover:bg-red-200"
                              >
                                Vote Against
                              </button>
                            </div>
                          )}
                        </Disclosure.Panel>
                      </Transition>
                    </>
                  )}
                </Disclosure>
              ))}
            </div>
          </Tab.Panel>

          {/* New Proposal Form */}
          <Tab.Panel>
            <div className="max-w-2xl mx-auto bg-white p-6 rounded-lg shadow">
              <h2 className="text-xl font-semibold mb-6">Create New Proposal</h2>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-1">
                    Title
                  </label>
                  <input
                    type="text"
                    value={newProposal.title}
                    onChange={(e) => setNewProposal({...newProposal, title: e.target.value})}
                    className="w-full px-3 py-2 border rounded-lg"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-1">
                    Description
                  </label>
                  <textarea
                    value={newProposal.description}
                    onChange={(e) => setNewProposal({...newProposal, description: e.target.value})}
                    className="w-full px-3 py-2 border rounded-lg h-32"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-1">
                    IPFS Metadata URI
                  </label>
                  <input
                    type="text"
                    value={newProposal.ipfsDetails}
                    onChange={(e) => setNewProposal({...newProposal, ipfsDetails: e.target.value})}
                    className="w-full px-3 py-2 border rounded-lg"
                  />
                </div>
                <button
                  onClick={createProposal}
                  className="w-full py-2 px-4 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700"
                >
                  Submit Proposal
                </button>
              </div>
            </div>
          </Tab.Panel>

          {/* Stake Tokens Form */}
          <Tab.Panel>
            <div className="max-w-md mx-auto bg-white p-6 rounded-lg shadow">
              <h2 className="text-xl font-semibold mb-6">Stake HAUNT Tokens</h2>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-1">
                    Amount to Stake
                  </label>
                  <input
                    type="number"
                    value={stakeAmount}
                    onChange={(e) => setStakeAmount(e.target.value)}
                    className="w-full px-3 py-2 border rounded-lg"
                    placeholder="0.00"
                  />
                </div>
                <button
                  onClick={stakeTokens}
                  className="w-full py-2 px-4 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700"
                >
                  Stake Tokens
                </button>
              </div>
            </div>
          </Tab.Panel>

          {/* Delegate Form */}
          <Tab.Panel>
            <div className="max-w-md mx-auto bg-white p-6 rounded-lg shadow">
              <h2 className="text-xl font-semibold mb-6">Delegate Voting Power</h2>
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-1">
                    Delegate Address
                  </label>
                  <input
                    type="text"
                    value={delegateAddress}
                    onChange={(e) => setDelegateAddress(e.target.value)}
                    className="w-full px-3 py-2 border rounded-lg"
                    placeholder="Haunti address"
                  />
                </div>
                <button
                  onClick={delegateVotingPower}
                  className="w-full py-2 px-4 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700"
                >
                  Delegate Votes
                </button>
              </div>
            </div>
          </Tab.Panel>
        </Tab.Panels>
      </Tab.Group>
    </div>
  )
}

export default Governance
