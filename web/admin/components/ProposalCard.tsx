import { useState, useEffect } from 'react'
import { PublicKey } from '@solana/web3.js'
import { ProposalState } from '@haunti/governance-sdk'
import { Disclosure, Transition } from '@headlessui/react'
import { ChevronUpIcon, ClockIcon, CheckCircleIcon, XCircleIcon, UserCircleIcon } from '@heroicons/react/24/outline'
import { formatDistanceToNow, format } from 'date-fns'
import { toast } from 'react-toastify'
import ProgressBar from './ProgressBar'

interface Proposal {
  publicKey: PublicKey
  title: string
  description: string
  state: ProposalState
  forVotes: BN
  againstVotes: BN
  createdAt: number
  creator: PublicKey
  startTime: number
  endTime: number
}

interface ProposalCardProps {
  proposal: Proposal
  votingPower: BN | null
  userVote: 'for' | 'against' | null
  onVote: (proposalId: PublicKey, approve: boolean) => Promise<void>
}

const STATE_COLORS = {
  [ProposalState.DRAFT]: 'bg-gray-100 text-gray-600',
  [ProposalState.ACTIVE]: 'bg-green-100 text-green-800',
  [ProposalState.SUCCEEDED]: 'bg-blue-100 text-blue-800',
  [ProposalState.DEFEATED]: 'bg-red-100 text-red-800',
  [ProposalState.EXECUTED]: 'bg-purple-100 text-purple-800',
  [ProposalState.CANCELED]: 'bg-yellow-100 text-yellow-800',
}

const ProposalCard: React.FC<ProposalCardProps> = ({ proposal, votingPower, userVote, onVote }) => {
  const [isVoting, setIsVoting] = useState(false)
  const [localUserVote, setLocalUserVote] = useState(userVote)
  const [localVotes, setLocalVotes] = useState({ for: proposal.forVotes, against: proposal.againstVotes })

  const totalVotes = localVotes.for.add(localVotes.against)
  const forPercentage = totalVotes.gtn(0) 
    ? localVotes.for.muln(100).div(totalVotes).toNumber()
    : 0
  const againstPercentage = 100 - forPercentage

  const handleVote = async (approve: boolean) => {
    if (!votingPower || votingPower.lten(0)) {
      toast.error('Insufficient voting power')
      return
    }

    setIsVoting(true)
    try {
      await onVote(proposal.publicKey, approve)
      setLocalUserVote(approve ? 'for' : 'against')
      setLocalVotes(prev => ({
        for: approve ? prev.for.add(votingPower!) : prev.for,
        against: !approve ? prev.against.add(votingPower!) : prev.against
      }))
      toast.success(`Vote ${approve ? 'for' : 'against'} recorded`)
    } catch (error) {
      toast.error(`Voting failed: ${error.message}`)
    } finally {
      setIsVoting(false)
    }
  }

  return (
    <Disclosure>
      {({ open }) => (
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 hover:border-indigo-200 transition-colors">
          <Disclosure.Button className="w-full px-4 py-3 text-left">
            <div className="flex items-center justify-between">
              <div className="flex-1 min-w-0">
                <div className="flex items-center space-x-3">
                  <span className={`px-2 py-1 rounded text-sm font-medium ${STATE_COLORS[proposal.state]}`}>
                    {ProposalState[proposal.state]}
                  </span>
                  <h3 className="text-lg font-medium truncate">{proposal.title}</h3>
                </div>
                <div className="mt-1 flex items-center space-x-4 text-sm text-gray-500">
                  <span className="inline-flex items-center">
                    <UserCircleIcon className="h-4 w-4 mr-1" />
                    {proposal.creator.toString().slice(0, 6)}...{proposal.creator.toString().slice(-4)}
                  </span>
                  <span className="inline-flex items-center">
                    <ClockIcon className="h-4 w-4 mr-1" />
                    {formatDistanceToNow(proposal.endTime * 1000, { addSuffix: true })}
                  </span>
                </div>
              </div>
              <ChevronUpIcon
                className={`${
                  open ? 'transform rotate-180' : ''
                } w-5 h-5 text-indigo-500 flex-shrink-0`}
              />
            </div>
          </Disclosure.Button>

          <Transition
            enter="transition duration-100 ease-out"
            enterFrom="transform scale-95 opacity-0"
            enterTo="transform scale-100 opacity-100"
            leave="transition duration-75 ease-out"
            leaveFrom="transform scale-100 opacity-100"
            leaveTo="transform scale-95 opacity-0"
          >
            <Disclosure.Panel className="px-4 pt-4 pb-2">
              <div className="border-t border-gray-100 pt-4">
                <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                  {/* Voting Section */}
                  <div>
                    <h4 className="text-sm font-semibold text-gray-700 mb-4">Voting Results</h4>
                    
                    <div className="space-y-4">
                      <div className="flex items-center justify-between">
                        <span className="flex items-center text-green-600">
                          <CheckCircleIcon className="h-5 w-5 mr-2" />
                          For
                        </span>
                        <span className="font-mono">
                          {(localVotes.for.toNumber() / 1e6).toLocaleString()} HAUNT
                        </span>
                      </div>
                      <ProgressBar value={forPercentage} color="bg-green-500" />

                      <div className="flex items-center justify-between">
                        <span className="flex items-center text-red-600">
                          <XCircleIcon className="h-5 w-5 mr-2" />
                          Against
                        </span>
                        <span className="font-mono">
                          {(localVotes.against.toNumber() / 1e6).toLocaleString()} HAUNT
                        </span>
                      </div>
                      <ProgressBar value={againstPercentage} color="bg-red-500" />
                    </div>

                    {proposal.state === ProposalState.ACTIVE && (
                      <div className="mt-6 space-y-3">
                        <button
                          onClick={() => handleVote(true)}
                          disabled={isVoting || !!localUserVote}
                          className="w-full flex items-center justify-center px-4 py-2 border border-green-500 text-green-500 rounded-lg hover:bg-green-50 disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          {localUserVote === 'for' ? (
                            <>
                              <CheckCircleIcon className="h-5 w-5 mr-2" />
                              Voted For
                            </>
                          ) : (
                            'Vote For'
                          )}
                        </button>

                        <button
                          onClick={() => handleVote(false)}
                          disabled={isVoting || !!localUserVote}
                          className="w-full flex items-center justify-center px-4 py-2 border border-red-500 text-red-500 rounded-lg hover:bg-red-50 disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          {localUserVote === 'against' ? (
                            <>
                              <XCircleIcon className="h-5 w-5 mr-2" />
                              Voted Against
                            </>
                          ) : (
                            'Vote Against'
                          )}
                        </button>
                      </div>
                    )}
                  </div>

                  {/* Proposal Details */}
                  <div>
                    <h4 className="text-sm font-semibold text-gray-700 mb-4">Proposal Details</h4>
                    <div className="prose prose-sm max-w-none text-gray-600">
                      {proposal.description}
                    </div>

                    <dl className="mt-6 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
                      <div>
                        <dt className="text-gray-500">Proposed</dt>
                        <dd className="font-medium text-gray-900">
                          {format(proposal.createdAt * 1000, 'MMM d, yyyy HH:mm')}
                        </dd>
                      </div>
                      <div>
                        <dt className="text-gray-500">Voting Ends</dt>
                        <dd className="font-medium text-gray-900">
                          {format(proposal.endTime * 1000, 'MMM d, yyyy HH:mm')}
                        </dd>
                      </div>
                      <div>
                        <dt className="text-gray-500">Total Votes</dt>
                        <dd className="font-medium text-gray-900">
                          {(totalVotes.toNumber() / 1e6).toLocaleString()} HAUNT
                        </dd>
                      </div>
                      <div>
                        <dt className="text-gray-500">Quorum</dt>
                        <dd className="font-medium text-gray-900">
                          {/* Replace with actual quorum logic */}
                          15,000 HAUNT
                        </dd>
                      </div>
                    </dl>
                  </div>
                </div>
              </div>
            </Disclosure.Panel>
          </Transition>
        </div>
      )}
    </Disclosure>
  )
}

export default ProposalCard
